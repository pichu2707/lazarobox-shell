# Delta for config-menu

Tags as in the main spec: [auto] = `cargo test`; [manual] = real Kitty or doc review.

## Base and archive order

The main spec trails the code. This delta restates every MODIFIED requirement against the MERGED state of the in-flight config-menu deltas (agent-tools, coding-tools, agent-orchestrator, auto-execute, hf-download, feedback, llama-server-install, llamacpp-pick-model, model-marks), with the code in `src/core/menu.rs` as tiebreaker where they disagree. Known disagreements settled by the code: the Settings tab has eight rows (`llama-server idle` and `Feedback` are real rows); `Approval` has the values `ask`, `auto`, `run`; the task list has seven task rows (`orchestrate`); the AI rows are `... Tools, Search, Write, Exec, Git, Git write, Approval, Orchestrator, Hugging Face`.

ai-menu-tabs MUST archive AFTER all of those changes. Archived earlier, a later merge would reintroduce the `AI` tab.

Purpose and Terms paragraph of the main spec: read "two tabs, Settings and AI" as "menu tabs Settings, Models, Tools, Connectors and Chat"; "Menu tab" lists those five. "AI section" keeps its meaning (the part of the menu attached by `with_ai`, which owns the AI draft).

## Tab map (normative)

| Tab | Digit | Rows, in order | Exists |
|---|---|---|---|
| Settings | 1 | `Statusline position`, `Tab bar position`, `Mouse`, `Notify`\*, `Sound`\*, `llama-server idle`\*, `Updates`, `Feedback` | always (without the AI section only the rows not marked \*) |
| Models | 2 | `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `Hugging Face` | AI section |
| Tools | 3 | `Tools`, `Search`, `Write`, `Exec`, `Git`, `Git write`, `Approval`, `Orchestrator` | AI section |
| Connectors | 4 | none today | AI section AND at least one row; hidden now |
| Chat | 5 | `Profile`, `Context`, `Memory` | AI section |

## MODIFIED Requirements

### Requirement: Open and close
The menu MUST open only from TERMINAL through the prefix: `Ctrl+Space` then `m`, always on the `Settings` tab, on its first row (`Statusline position`), with the per-tab cursor memory cleared and the picker closed. Opening MUST NOT write to any PTY. The menu MUST close on `s` (after a successful save, or at once when nothing changed) and on Esc at the top level of the task list (after reverting). Esc while a picker is open MUST go back exactly one level (custom field to level 2, level 2 to level 1, level 1 to the task list) and MUST NOT close the menu or revert the draft. This Esc rule takes precedence wherever Esc is described. While open the mode is MENU (see modal-input). Opening captures the current values of the menu-owned settings as the "original" values and as the initial draft.
(Previously: scenarios named the `AI` tab; text unchanged)

#### Scenario: Open [auto]
- GIVEN TERMINAL
- WHEN `Ctrl+Space` then `m` are pressed
- THEN mode is MENU, the menu is visible on the Settings tab with the first row selected, and no WritePty is emitted

#### Scenario: Esc closes [auto]
- GIVEN MENU with no changes
- WHEN Esc is pressed
- THEN mode is TERMINAL, the menu is gone, no file is touched and no WritePty is emitted

#### Scenario: Reopen starts fresh [auto]
- GIVEN the menu was opened, changed, and cancelled with Esc
- WHEN it is opened again
- THEN the active tab is `Settings`, the selection is on its first row and the draft equals the current (reverted) values

#### Scenario: Reopen resets tab and cursor [auto]
- GIVEN the menu closed while the Models tab was active with `code` selected, and the Settings cursor had been moved to `Mouse`
- WHEN the menu is reopened
- THEN the active tab is `Settings`, the selected row is `Statusline position`, after Tab then Shift+Tab the Settings row is again the first, not `Mouse`, and after `2` the Models row is `Set all tasks`, not `code`

#### Scenario: Esc in the picker goes back one level [auto]
- GIVEN the picker is open at level 2 for `code` after changing the Statusline position in the draft
- WHEN Esc is pressed, then Esc again, then Esc a third time
- THEN level 1 shows, then the task list shows on the Models tab with `code` selected and the draft unchanged, then the menu closes and the Statusline position is reverted

#### Scenario: Reopen after picker is on the task list [auto]
- GIVEN the menu closed with Esc while no picker was open, after a picker was used earlier
- WHEN the menu is reopened
- THEN the task list shows on the Settings tab, the picker is closed and the draft equals the current values

### Requirement: Sections and items are data
Sections, items and tabs MUST come from static descriptor tables; adding a section, a tab or a setting MUST NOT require changing the popup drawing or navigation code. Each setting item MUST declare its label and its `config.toml` key path where one exists, and either an ordered list of allowed values (static item) or a task kind (assignment item, whose value is an `engine › model` chosen in the picker). Each row MUST belong to exactly one tab, as in the Tab map: `Settings` (`Statusline position` `statusline.position`: top, bottom; `Tab bar position` `tabbar.position`: top, bottom; `Mouse` `mouse`: on, off, written as the booleans `true` and `false`; `Notify`: auto, desktop, bell, off; `Sound`: off, on; `llama-server idle` `ai.servers.llamacpp.idle_minutes`: 5 min, 15 min, 30 min, never, written as a TOML integer, never = 0; `Updates`: the running version, or the newer one when known; `Feedback`: `bug report or idea`, no key path), `Models` (`Set all tasks` aggregate over the five tasks, no single key path; `general` `ai`; `code` `ai.tasks.code`; `chat` `ai.tasks.chat`; `review` `ai.tasks.review`; `summarize` `ai.tasks.summarize`; `orchestrate` its task table; `Hugging Face`, no key path), `Tools` (`Tools` `ai.tools.enabled`: off, on; `Search`, `Write`, `Exec`, `Git`, `Git write`: off, on, each with its own `ai.tools.<key>`; `Approval` `ai.tools.approval`: ask, auto, run; `Orchestrator` `ai.orchestrator.enabled`: off, on), `Connectors` (no rows) and `Chat` (`Profile` `ai.profile`: analyst, programmer; `Context` `ai.context`: off, on; `Memory` `ai.memory.enabled`: off, on). `Notify`, `Sound` and `llama-server idle` edit the AI draft, so they MUST NOT be shown (and MUST NOT be selectable) when the AI section is not attached; then `Settings` holds `Statusline position`, `Tab bar position`, `Mouse`, `Updates` and `Feedback`, and no other tab exists. Moving a row between tabs MUST NOT change its key path, its values or the config edit it produces.
(Previously: two tabs, `Settings` with seven rows and a 19-row `AI` tab; the Settings list omitted `Feedback` and the no-AI list omitted `Feedback`)

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor tables
- WHEN they are validated
- THEN every tab has a non-empty label, every item a non-empty label, every static item with a key path has at least two allowed values, `Mouse` has the key path `mouse` and the values `on`, `off`, the task rows have the task ids of ai-task-models and a key path, `Set all tasks` has no single key path, `Profile`, `Context`, `Memory`, `Tools`, `Approval` and the five family rows have their key paths and values above, every tab lists only rows that exist, every row appears on exactly one tab, and there are no duplicate key paths

#### Scenario: Settings rows [auto, TestBackend + insta]
- GIVEN the menu just opened with the AI section attached and `mouse` at its default
- WHEN rendered on a normal terminal
- THEN, with no `Settings` title row (the bar names the tab), the Settings tab shows `Statusline position`, `Tab bar position`, `Mouse` (`on`, not muted), `Notify`, `Sound`, `llama-server idle` (`15 min`), `Updates` and `Feedback`, each with its current value, and no Models, Tools or Chat row is drawn

#### Scenario: Every row is on its tab [auto, TestBackend + insta]
- GIVEN the AI section attached
- WHEN each of Models, Tools and Chat is rendered
- THEN Models reads `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `Hugging Face`; Tools reads `Tools`, `Search`, `Write`, `Exec`, `Git`, `Git write`, `Approval`, `Orchestrator`; Chat reads `Profile`, `Context`, `Memory`; and no `Notify`, `Sound`, `Updates` or `Feedback` row exists on them

#### Scenario: No AI keeps one tab [auto]
- GIVEN the menu without the AI section
- WHEN the rows are built
- THEN the Settings tab holds `Statusline position`, `Tab bar position`, `Mouse`, `Updates` and `Feedback`, and there is no other tab

### Requirement: Navigation
`j`, `k`, Down and Up MUST move the selection between the selectable rows of the ACTIVE tab only, in display order, and MUST wrap around at the ends of that tab (from its last row to its first and back); they MUST NOT cross to another tab, because Tab, Shift+Tab and the jump keys are the only way across tabs. No row is skipped (picker disabled rows are different, see Task picker). Repeat events MUST be handled like Press. The order of each tab is the one in the Tab map: `Settings` ends with `Updates`, `Feedback`; `Models` ends with `orchestrate`, `Hugging Face`; `Tools` ends with `Approval`, `Orchestrator`; `Chat` is `Profile`, `Context`, `Memory`.
(Previously: two tabs; the `AI` tab order ran from `Set all tasks` to `Hugging Face` in one list; the Settings order omitted `llama-server idle` and `Feedback`)

#### Scenario: Move down and up [auto]
- GIVEN MENU with the first row selected
- WHEN `j` is pressed, then `k`
- THEN the second setting row is selected, then the first again

#### Scenario: Wrap within Settings [auto]
- GIVEN the `Feedback` row selected on Settings
- WHEN `j` (or Down) is pressed
- THEN `Statusline position` is selected and the active tab is still `Settings`; and from the first row `k` (or Up) selects `Feedback`

#### Scenario: Wrap within Models, Tools and Chat [auto]
- GIVEN each of Models (`Hugging Face` selected), Tools (`Orchestrator` selected) and Chat (`Memory` selected) in turn
- WHEN `j` is pressed, then `k`
- THEN the first row of that tab (`Set all tasks`, `Tools`, `Profile`) is selected, then the last again, and the active tab does not change

#### Scenario: Mouse row is selectable [auto]
- GIVEN the Tab bar position row selected
- WHEN `j` is pressed
- THEN `Mouse` is selected, `j` again selects `Notify`; from `Notify` `k` goes back to `Mouse`

#### Scenario: Task rows in order [auto]
- GIVEN the Models tab active with `Set all tasks` selected
- WHEN `j` is pressed eight times
- THEN the selection visits `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `Hugging Face` in that order and the eighth press wraps to `Set all tasks`

#### Scenario: Tools tab in order [auto]
- GIVEN the Tools tab active with `Tools` selected
- WHEN `j` is pressed eight times
- THEN the selection visits `Search`, `Write`, `Exec`, `Git`, `Git write`, `Approval`, `Orchestrator` in that order and the eighth press wraps to `Tools`

#### Scenario: Repeat moves [auto]
- GIVEN MENU
- WHEN a Repeat event for `j` arrives
- THEN the selection moves like a Press

### Requirement: Input is swallowed
In MENU every key not listed above MUST be swallowed with no effect. The listed keys are `j`, `k`, Up, Down, `h`, `l`, Left, Right, Space, Enter, `s`, `r`, `t`, `y` (on the Updates command and the warning), `d` (on a Hugging Face row with a saved token), Tab, Shift+Tab (`BackTab` with or without SHIFT, and Tab with SHIFT), the jump keys `1` to `5` (unmodified, at the top level only) and Esc, each with the behavior described in this spec, and inside the custom field typed characters, Backspace and Ctrl+U; in the Hugging Face token field and the Feedback form, Ctrl+O opens their link (see those requirements). Ctrl+Tab and Alt+Tab are unlisted and swallowed; so is any digit with Ctrl or Alt (modified keys stay swallowed as today). At the top level `0` and `6` to `9` are not swallowed silently: they show the footer notice `Tabs are 1-5` (see Jump keys), and Tab or Shift+Tab with a single visible tab shows `No other tabs`. Tab, Shift+Tab and the jump keys are listed at the top level only; in the picker, the custom field and the warning they follow Sub-levels keep the bar inert. The PTY MUST NOT receive any key or paste while the menu is open. Release events MUST be ignored. Ctrl+Space MUST be swallowed (not cancel, not literal NUL), also inside the custom field. Shift is ignored for listed keys (Shift+Space cycles, Shift+Enter cycles or selects like Enter); uppercase letters such as `J`, `S` and `R` are unlisted and swallowed outside the custom field. `r` is listed only for the refresh behavior (see Refresh key) and `s` only for saving (see config-persistence).
(Previously: no digit was listed; `d` was not mentioned)

#### Scenario: Unknown key [auto]
- GIVEN MENU
- WHEN `x` or Ctrl+Space is pressed
- THEN no WritePty is emitted and mode stays MENU

#### Scenario: Tab is listed at the top level [auto]
- GIVEN MENU at the top level with the AI section attached
- WHEN Tab is pressed
- THEN the active tab changes, no WritePty is emitted and mode stays MENU

#### Scenario: Out-of-range digits show a notice, modified digits are swallowed [auto]
- GIVEN MENU at the top level on Settings
- WHEN `0`, `6` and `9` are pressed, then Ctrl+`2` and Alt+`3`
- THEN the first three each show the footer notice `Tabs are 1-5`; Ctrl+`2` and Alt+`3` change nothing and show no notice; in all cases the active tab and the draft are unchanged, no WritePty is emitted and mode stays MENU

#### Scenario: Paste ignored [auto]
- GIVEN MENU with the custom field not focused
- WHEN a paste event arrives
- THEN no WritePty is emitted and the menu is unchanged

#### Scenario: Uppercase S is swallowed [auto]
- GIVEN the task list with a changed draft
- WHEN `S` is pressed
- THEN no save is requested and the menu is unchanged

### Requirement: Tiny terminals
The menu MUST never panic at any terminal size down to 0x0, on every visible tab and in every picker view. The popup width is 62 with the AI section (51 without) and its height is the fixed popup height (see Popup height is fixed: `MENU_BODY` 12 plus `CHROME` 4 rows of chrome: two borders, the blank row and the footer, so 16). The full popup needs at least 24 columns by 15 rows, the smallest full popup (the blank row is the first thing dropped); adding a row to the tallest view grows these numbers. Below that the menu MUST draw only a one-line fallback message instead of the full popup; the key handling MUST be unchanged, so Esc still reverts and closes (or goes back one level when a picker is open), `s` still saves, and Tab, Shift+Tab and the jump keys still switch tabs. Content that does not fit MUST be clipped, including the menu tab bar, which narrows by the steps of Menu tab bar.
(Previously: scenarios named the `AI` tab; the bar degraded only as far as the hint)

#### Scenario: No panic at any size [auto]
- GIVEN the menu open on each visible tab, then in each picker view and the warning
- WHEN rendered at sizes from 0x0 up through small widths and heights
- THEN nothing panics

#### Scenario: Minimal message and Esc works [auto]
- GIVEN a terminal smaller than the menu minimum and the menu open
- WHEN rendered, then Esc is pressed
- THEN a minimal message is shown instead of the popup and Esc reverts and closes

#### Scenario: Minimal message and s saves [auto]
- GIVEN a terminal smaller than the menu minimum, the menu open and a changed draft
- WHEN `s` is pressed
- THEN a save is requested

#### Scenario: Tabs still switch under the fallback [auto]
- GIVEN a terminal smaller than the menu minimum and the AI section attached
- WHEN Tab, then `3` are pressed
- THEN the active tab is `Models`, then `Tools`

#### Scenario: Picker too small keeps keys [auto]
- GIVEN a terminal one column narrower than 24, and one 24 wide but 14 rows high, and the picker open at level 1
- WHEN rendered, then Esc is pressed
- THEN the one-line fallback is shown, then the task list view is the current view

#### Scenario: Smallest full popup [auto, TestBackend + insta]
- GIVEN a terminal of exactly the minimum size (24 columns by 15 rows)
- WHEN the Settings, Models, Tools and Chat tabs and the picker are rendered
- THEN the full popup is drawn (not the fallback), clipped, without panic, with the bar reduced to the active tab

#### Scenario: Fallback threshold follows the popup [auto]
- GIVEN a terminal one column or one row smaller than the popup minimum, and one exactly at it
- WHEN rendered
- THEN the first shows the one-line fallback and the second shows the full popup

### RENAMED Requirements

### Requirement: AI task list → Models tab

(Reason: the task rows now live on their own tab; the static rows that followed them moved to Tools and Chat)
(Migration: tests and docs that say "AI task list" or "the AI tab" use "Models tab"; Profile, Context and Memory are on Chat, the rest on Tools)

### Requirement: Models tab
The Models tab MUST show, as its rows (with no section title row while the tab bar is shown), exactly seven task rows in this order: `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, and then the static row `Hugging Face` (see Hugging Face account row). Each task row shows its label and its value (see ai-task-models): `<engine> › <model>`, `(default)` for an unassigned non-general task, `(not set)` for an unset `general` or an unset aggregate, `(mixed)` for the aggregate when the tasks it covers differ, and the marks `· download required`, `(not detected)` and, on `orchestrate` only, `· not recommended` as defined there. The `Hugging Face` row shows its account state. The number of rows MUST never change because of a detection result. The popup MUST show every row of the active tab without clipping on a normal terminal, MUST keep the same size across all tabs and the picker, and the minimum terminal size below which only the one-line fallback is drawn (see Tiny terminals) MUST follow the popup. The detection cues, the `r` refresh key and the task hint apply on the task rows only; on the `Hugging Face` row, on Tools and on Chat rows `r` requests no detection run.
(Previously: "AI task list", one tab of 19 rows that also held Profile, Context, Memory, Tools, the family switches, Approval and Orchestrator)

#### Scenario: Task rows rendered [auto, TestBackend + insta]
- GIVEN general is `claude` / `sonnet`, code is `ollama` / `llama3:8b` (ollama available with that model), the other tasks unassigned, no Hugging Face token, and the Models tab active
- WHEN the menu is rendered on a normal terminal
- THEN the rows are visible: `Set all tasks` `(mixed)`, `general` `claude › sonnet`, `code` `ollama › llama3:8b`, `chat` `(default)`, `review` `(default)`, `summarize` `(default)`, `orchestrate` `(default)`, `Hugging Face` `not connected`, and no `Profile`, `Context`, `Memory`, `Tools` or `Orchestrator` row

#### Scenario: Nothing assigned [auto, TestBackend + insta]
- GIVEN no task is assigned and the Models tab active
- WHEN the menu is rendered
- THEN `Set all tasks` reads `(not set)`, `general` reads `(not set)` and the other five read `(default)`

#### Scenario: Marks on rows [auto]
- GIVEN code is `ollama` / `old:7b` not downloaded, and review is `codex` / `default` with codex `not installed`
- WHEN the Models tab is rendered
- THEN the rows read `ollama › old:7b · download required` and `codex (not detected) › default`

#### Scenario: Refresh only on task rows [auto]
- GIVEN the Models tab with `Hugging Face` selected, then the Tools tab with `Approval` selected
- WHEN `r` is pressed in each
- THEN no detection run is requested; and with `code` selected exactly one is

#### Scenario: Popup size is stable [auto, TestBackend + insta]
- GIVEN the Settings, Models, Tools and Chat tabs and then the picker open on a normal terminal
- WHEN each is rendered
- THEN the popup has the same size in all and every row of the active tab is visible

#### Scenario: Fallback threshold follows the popup [auto]
- GIVEN a terminal one column or one row smaller than the popup minimum, and one exactly at it
- WHEN rendered
- THEN the first shows the one-line fallback and the second shows the full popup without panic

### Requirement: Notify row
`Notify` is a static row of the `Settings` tab (after `Mouse`, before `Sound`), found by `ItemKind::Notify`, never by position. It edits the AI draft, so it is shown only when the AI section is attached. It reads one of four modes from the AI draft: `auto`, `desktop`, `bell`, `off`. `h`, `l`, arrows, Space and Enter cycle the mode with wrap (an Enter key repeat is ignored like on `Context`), with no relayout and no other effect. `t` tests the notification (see Notify, Sound and Updates keys). The footer is `j/k move · h/l change · t test · s save · Esc cancel`. Only a changed value is an edit (`ai.notify`), placed after the memory edits.
(Previously: the order scenario stopped at `Updates` and the snapshot scenario named the `AI` tab)

#### Scenario: Row order and navigation [auto]
- GIVEN the menu with the AI section, Settings tab active
- WHEN navigating down from `Mouse`
- THEN the selection reaches `Notify`, then `Sound`, then `llama-server idle`, then `Updates`, then `Feedback`, then wraps to `Statusline position`

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
- THEN `Notify` and `Sound` appear on the Settings tab and not on Models, Tools or Chat

### Requirement: Menu tabs
The popup MUST group its rows into menu tabs. The tabs MUST come from a static table (label, the rows it shows, whether it needs the AI section, whether it is hidden while it has no rows); adding a tab MUST NOT require changing navigation or drawing code. The table order is `Settings`, `Models`, `Tools`, `Connectors`, `Chat`. Without the AI section only `Settings` exists. With it, `Models`, `Tools` and `Chat` exist and `Connectors` is visible only while it has at least one row; it has none today, so it MUST NOT be drawn, MUST NOT be reached by Tab or Shift+Tab, and MUST appear, in its table position between `Tools` and `Chat`, as soon as a row is declared for it (MCP, agent-tools 4a). `Tab` MUST activate the next visible tab; `Tab` with SHIFT and `BackTab` (with or without SHIFT) MUST activate the previous one. Wrap is across the visible tabs (Settings, Models, Tools, Chat, Settings while Connectors is hidden). Ctrl+Tab and Alt+Tab MUST stay swallowed. With one visible tab, Tab and Shift+Tab change nothing and show the footer notice `No other tabs` (cleared by the next key press, like the jump-key notice). A held key repeats like `j`/`k`: Repeat events switch tabs like Press. The cursor MUST be remembered per tab, keyed by the tab and not by its place among the visible ones: leaving a tab and coming back MUST restore the row that was selected there, and a tab becoming visible MUST NOT move the memory of another. Each tab with at least one row starts on its first row. Switching tabs, by Tab or by a jump key, MUST NOT change any draft value and MUST NOT emit `ResizePty`, `WritePty` or a detection run.
(Previously: tabs `Settings` and `AI`; "An `MCP` tab MUST NOT be listed until agent-tools 4a adds its table entry")

#### Scenario: Tab cycles forward with wrap [auto]
- GIVEN the AI section attached and the active tab `Settings`
- WHEN Tab is pressed four times
- THEN the active tab is `Models`, `Tools`, `Chat`, then `Settings` (Connectors is skipped)

#### Scenario: Shift+Tab, both encodings [auto]
- GIVEN the active tab `Settings`
- WHEN `BackTab` is pressed, in a separate run `BackTab` with SHIFT, and in another `Tab` with SHIFT
- THEN in all runs the active tab is `Chat`; and pressing the same key again gives `Tools`

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
- GIVEN the tab table and the AI section attached, and no row declared for Connectors
- WHEN the tabs are listed
- THEN they are exactly `Settings`, `Models`, `Tools`, `Chat`, in that order

#### Scenario: Connectors appears with its first row [auto]
- GIVEN a test table whose Connectors tab has one row, and the AI section attached
- WHEN the tabs are listed, and Tab is pressed from `Tools`, then again
- THEN they are `Settings`, `Models`, `Tools`, `Connectors`, `Chat`, and the active tab is `Connectors`, then `Chat`

#### Scenario: A tab appearing keeps the cursors [auto]
- GIVEN `Chat` with `Memory` selected and Connectors hidden
- WHEN a row is declared for Connectors and the menu is rebuilt with the same memory, then `5` is pressed
- THEN `Chat` is active with `Memory` still selected

#### Scenario: No AI, one tab [auto]
- GIVEN the menu without the AI section
- WHEN Tab and Shift+Tab are pressed
- THEN the tab does not change, the footer shows `No other tabs` and nothing else happens

### Requirement: Jump keys
At the top level, an unmodified `1` to `5` MUST jump to a tab by its FIXED position in the tab table: `1` Settings, `2` Models, `3` Tools, `4` Connectors, `5` Chat. The digits do not renumber when Connectors is hidden. Press and Repeat act alike. A jump to the visible tab that is not active MUST activate it with the same cursor memory as Tab. A jump to the active tab MUST change nothing and show no notice. A jump to a tab that does not exist MUST show a visible one-line notice that names the reason and MUST change nothing else: `4` while Connectors has no rows (the notice says there are no connectors yet), and `2` to `5` without the AI section (the notice says the AI section is not available). The notice is drawn in the footer with the precedence of an error, is cleared by the next key press (which then acts normally), blocks neither `s` nor Esc, and writes nothing to any PTY. `0` and `6` to `9` (unmodified, top level) show the notice `Tabs are 1-5` and change nothing else. Digits with Ctrl or Alt stay swallowed. No unmodified digit is ever swallowed without an effect or a notice at the top level.

#### Scenario: Jump to each tab [auto]
- GIVEN the AI section attached and a test table with Connectors visible
- WHEN `1`, `2`, `3`, `4`, `5` are pressed in turn
- THEN the active tab is Settings, Models, Tools, Connectors, Chat

#### Scenario: Digits keep their positions [auto]
- GIVEN the AI section attached and Connectors hidden
- WHEN `5` is pressed
- THEN the active tab is `Chat`, not `Tools` or a fourth tab

#### Scenario: Hidden Connectors shows a notice [auto, TestBackend + insta]
- GIVEN Connectors hidden and the active tab `Models` with `code` selected
- WHEN `4` is pressed
- THEN the active tab is still `Models` with `code` selected, the footer shows a notice about the missing connectors instead of the hint, and the next key press clears it

#### Scenario: No AI section shows a notice [auto]
- GIVEN the menu without the AI section
- WHEN `2`, then `3`, then `4`, then `5` are pressed
- THEN each shows the notice that the AI section is not available, the tab does not change and nothing is written

#### Scenario: Active tab and Settings [auto]
- GIVEN the active tab `Models`
- WHEN `2` is pressed, then `1`
- THEN the first changes nothing and shows no notice, and the second activates `Settings`

#### Scenario: The notice does not block [auto]
- GIVEN the notice from `4` is showing and a draft is changed
- WHEN `s` is pressed, and in another run Esc
- THEN the first requests a save and the second reverts and closes

#### Scenario: Jump keys in Kitty [manual, Kitty]
- GIVEN the menu in Kitty with the AI section
- WHEN Tab, Shift+Tab and `1` to `5` are pressed
- THEN the tabs switch as specified, `4` shows the notice, and nvim receives no keys

### Requirement: Menu tab bar
The popup MUST draw a one-line menu tab bar as the first body row whenever there are two or more visible tabs. Each label is padded with one space on each side and labels are joined by `│`: BAR = sum of the label cells + 3 x tabs - 1. With the four visible tabs the bar ` Settings │ Models │ Tools │ Chat ` is 34 cells; with Connectors visible, ` Settings │ Models │ Tools │ Connectors │ Chat ` is 47; the `*` of `Settings*` adds one. The active tab MUST be drawn bold, `bg_base` on `primary_cyan` (like the main tab bar, and not like the selected row); idle tabs, the separator and the hint are `text_muted`. The hint `⇥ Tab · 1-5` (11 cells) is right-aligned and ends one column before the inner right edge. With W = inner width (popup width minus 2, where the popup width is the terminal width up to 62), the bar MUST degrade in this order: bar plus hint when W >= BAR + 13; bar only when BAR <= W; only the active tab as ` ‹ Models › ` (its label between `‹` and `›`), clipped, when W < BAR. Below 24 columns the existing one-line fallback applies. The active-only step is reachable in the full popup (a 5-tab bar below 49 columns, or below 50 with `Settings*`; a 4-tab bar below 36 columns, or below 37 with `Settings*`) and MUST be covered by end-to-end scenarios. The bar MUST be clipped to the popup and MUST NOT panic at any size. While an update is waiting (see Updates row) the `Settings` label MUST read `Settings*`. With a single tab (the no-AI path) the bar MUST be hidden and the popup layout MUST be unchanged from before this change. The hint is hidden while a sub-level is open (see Sub-levels keep the bar inert). With tabs shown, section titles are not drawn (the bar names the tab); with the bar hidden, section titles are drawn as before.
(Previously: two tabs, hint `⇥ Tab` of 5 cells shown from W >= BAR + 7, and the active-only step a guard that three or fewer tabs could not reach)

#### Scenario: Active tab highlighted [auto, TestBackend + insta]
- GIVEN the menu open on a normal terminal, active tab `Settings`, then Tab pressed
- WHEN each state is rendered
- THEN the bar lists `Settings`, `Models`, `Tools`, `Chat`, with `Settings` in the active style in the first and `Models` in the second, idle tabs and the `⇥ Tab · 1-5` hint in `text_muted`, and no section title rows are drawn

#### Scenario: Standard width, four tabs [auto, TestBackend + insta]
- GIVEN Connectors hidden, no update, a terminal 80 columns wide
- WHEN rendered
- THEN the bar (34) and the hint `⇥ Tab · 1-5` are drawn, the hint ending one column before the inner right edge

#### Scenario: Standard width, five tabs [auto, TestBackend + insta]
- GIVEN a test table with Connectors visible, no update, a terminal 80 columns wide (W = 60)
- WHEN rendered
- THEN the 47-cell bar and the hint are both drawn (W = 60 = BAR + 13)

#### Scenario: Five tabs and an update drop the hint [auto, TestBackend + insta]
- GIVEN Connectors visible and an update waiting (BAR = 48), a terminal 80 columns wide (W = 60 < 61)
- WHEN rendered
- THEN the bar reads ` Settings* │ Models │ Tools │ Connectors │ Chat ` without the hint

#### Scenario: Thresholds of the five-tab bar [auto]
- GIVEN Connectors visible and no update
- WHEN rendered at 49, 48 and 62 columns
- THEN 49 columns (W = 47) draws the whole bar without hint, 48 columns (W = 46) draws only ` ‹ <active> › `, and 62 columns draws the bar and the hint

#### Scenario: Thresholds of the four-tab bar [auto]
- GIVEN Connectors hidden and no update
- WHEN rendered at 49, 48, 36 and 35 columns
- THEN 49 columns draws the bar and the hint, 48 columns the bar without hint, 36 columns (W = 34) the bar without hint, and 35 columns (W = 33) only the active tab

#### Scenario: Bar at 24 columns [auto, TestBackend + insta]
- GIVEN the AI section attached, a terminal 24 columns wide and 15 rows high
- WHEN rendered
- THEN the full popup is drawn with the bar reduced to ` ‹ Settings › `, clipped inside the popup, and Tab changes the label

#### Scenario: Active-only guard [auto]
- GIVEN the bar line function with labels whose bar is wider than W
- WHEN it is built
- THEN it yields only the active tab as ` ‹ label › `, clipped to W, without panic

#### Scenario: Fallback at 22 columns [auto]
- GIVEN the menu open with the AI section
- WHEN rendered at a terminal 22 columns wide
- THEN the one-line fallback is drawn and no bar

#### Scenario: Settings marked while an update waits [auto]
- GIVEN a newer version known and not yet installed, active tab `Chat`
- WHEN rendered
- THEN the bar reads `Settings*` for the Settings label

#### Scenario: No update, no marker [auto]
- GIVEN no newer version known
- WHEN rendered
- THEN the label reads `Settings` without `*`

#### Scenario: Single tab hides the bar [auto, TestBackend + insta]
- GIVEN the menu without the AI section (one tab)
- WHEN rendered
- THEN no bar is drawn, Tab and Shift+Tab change nothing and show the notice `No other tabs`, and the single-tab snapshots show no bar and no `⇥` hint

### Requirement: Sub-levels keep the bar inert
While a sub-level is open (privacy warning, picker level 1 or 2 and its screens, custom field, Hugging Face token field and its prompts, Feedback form) the menu tab bar MUST stay drawn with the same styles, with the active tab unchanged, but WITHOUT the `⇥ Tab · 1-5` hint (it would advertise keys that are swallowed), and MUST be inert: Tab and Shift+Tab (both encodings) and the digits `1` to `5` MUST NOT switch tabs and MUST NOT show a notice there. In the picker lists they are swallowed with no effect. In every text entry (custom field, Hugging Face token field, Feedback fields) a digit MUST be typed like any other character and Tab keeps the meaning that view gives it (the Feedback form moves between fields). The privacy warning MUST keep declining on any key other than an unmodified lowercase `y`, Tab, Shift+Tab and the digits included, and closes only the warning view.
(Previously: Tab only, active tab named `AI`; the digits did not exist)

#### Scenario: Tab and digits inert in the picker [auto]
- GIVEN the picker open at level 1, then at level 2
- WHEN Tab, `BackTab`, Tab+SHIFT and `2` are pressed
- THEN the picker view, its cursor, the active tab and the draft are unchanged, no notice shows, the bar is still drawn and it has no hint

#### Scenario: Custom field types digits and ignores Tab [auto]
- GIVEN the custom field focused with buffer `ab`
- WHEN `1`, Tab and BackTab are pressed
- THEN the buffer is `ab1`, the field stays focused and the active tab is unchanged

#### Scenario: Token field and Feedback form type digits [auto]
- GIVEN the Hugging Face token field focused, and in another run the Feedback title field
- WHEN `4` is pressed
- THEN the character is typed in the field and the active tab is unchanged

#### Scenario: Tab and digits decline the warning [auto]
- GIVEN the privacy warning shown, active tab `Chat`
- WHEN Tab is pressed, and in another run `5`
- THEN the warning closes, the draft stays `off`, nothing is acknowledged, and the active tab is still `Chat`

### Requirement: Popup height is fixed
The popup body height MUST be the maximum over every view: each visible tab's rows, the picker (title plus its viewport) and the privacy warning. The popup height MUST be that body plus the chrome and MUST NOT change when switching tabs or entering or leaving a picker, the custom field or the warning. The tab bar row is part of the body. The body (`MENU_BODY`) is 12 rows: a 1-row bar plus 11 rows for the tallest view (`VIEW_BODY`). The chrome stays 4 rows, so the popup is 16 rows (it was 25 with the 19-row `AI` tab). The views need: the picker 11 (title plus its 10-row viewport), the Settings tab 11 (eight rows, one gap, the two-line note), the Tools tab 9 (eight rows and the reserved caution line), the privacy warning 10, the Models tab 8, the Chat tab 3 and the custom entry 2; the picker and the Settings tab are the binding 11, so the minimum stays 24 columns by 15 rows. Adding one row to the Settings tab (the ai-cost `Usage (alpha)` row) makes the Settings tab 12 and the popup 17 rows. The single-tab (no-AI) layout keeps its previous size.
(Previously: `AI` tab of 19 rows plus the caution made the body 21 and the popup 25)

#### Scenario: Same height across views [auto, TestBackend + insta]
- GIVEN a normal terminal
- WHEN the Settings, Models, Tools and Chat tabs, a picker level 1 and 2, the custom field and the warning are rendered
- THEN the popup border spans 16 rows in all of them

#### Scenario: Adding a row grows only the tallest view [auto]
- GIVEN the height is derived from the tables, with the picker and the Settings tab both at 11
- WHEN the tallest view is measured, and then a 9th Settings row is added
- THEN the fixed body equals the tallest view and no view exceeds it, and with the 9th Settings row the popup grows by one row; adding a row to Chat or Models does not change the height

### Requirement: Update note is Settings-only
The two-line update note MUST be drawn only on the Settings tab, inside the fixed body in its last 2 rows (just above the blank row and the footer), and MUST NOT appear on Models, Tools, Connectors or Chat or in any sub-level. The note MUST NOT make the popup taller than the fixed height and MUST NOT trigger the fallback. On the single-tab path the note keeps its previous position under the list.
(Previously: "MUST NOT appear on the AI tab")

#### Scenario: Note on Settings [auto, TestBackend + insta]
- GIVEN a newer version known, active tab `Settings`
- WHEN rendered
- THEN the update note shows in the last 2 body rows, above the blank row and the footer

#### Scenario: No note elsewhere [auto]
- GIVEN a newer version known, active tab `Models`, then `Tools`, then `Chat`
- WHEN rendered
- THEN the note text is absent and the popup height equals its height on Settings

### Requirement: Tools row
The Tools tab MUST hold a static row `Tools` as its first row, before the family rows and `Approval` (key path `ai.tools.enabled`, values `off`, `on`; it reads `on` by default). It is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter toggle the draft (an Enter repeat is ignored, like on `Context`); the footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and `r` do not apply. Changing the draft MUST have no live effect: no relayout, no ResizePty or WritePty, no change to a stream, a pending approval or a running tool, and it applies from the next turn after a successful save. Esc at the top level restores the draft; `s` saves `ai.tools.enabled` through config-persistence only when it differs. The row order of the tab is `Tools, Search, Write, Exec, Git, Git write, Approval, Orchestrator` (see Sections and items are data); Navigation wraps inside the Tools tab; the popup does not change size (see Popup height is fixed); the descriptor integrity rules extend to the row (`Tools` has key path `ai.tools.enabled`, values `off`, `on`, no duplicate key path).
(Previously: "a static row after `Memory` and before `Approval`" on the AI tab; Navigation wrapped inside the AI tab)

#### Scenario: Navigation reaches Tools and wraps [auto]
- GIVEN the Tools tab active with `Orchestrator` selected
- WHEN `j` is pressed
- THEN `Tools` is selected, the active tab is still `Tools`, and from `Tools` `k` selects `Orchestrator`

#### Scenario: Toggle [auto]
- GIVEN `Tools` selected with the draft `on`
- WHEN `l` is pressed, then `h`
- THEN the draft is `off`, then `on`, no save is requested and no file is touched

#### Scenario: Enter repeat ignored [auto]
- GIVEN `Tools` selected
- WHEN a Repeat event for Enter arrives
- THEN the draft is unchanged

#### Scenario: Footer and refresh [auto, TestBackend + insta]
- GIVEN `Tools` selected and detection in flight
- WHEN rendered, and `r` is pressed
- THEN the footer reads `j/k move · h/l change · s save · Esc cancel` and no detection run is requested

#### Scenario: No side effects on change [auto]
- GIVEN a running stream with a pending approval and the menu open on `Tools`
- WHEN the draft is cycled to `off`
- THEN no ResizePty or WritePty is emitted, the stream and the approval are untouched

#### Scenario: Applies from the next turn [auto]
- GIVEN the draft turned `off` and saved while a turn is running
- WHEN the next turn starts
- THEN the request is built without tools, and the running turn was not interrupted

#### Scenario: Esc reverts [auto]
- GIVEN the menu opened with tools `on` and the draft changed to `off`
- WHEN Esc is pressed on the task list
- THEN tools are `on` and no file is read or written

#### Scenario: Save [auto]
- GIVEN the draft changed to `off`
- WHEN `s` is pressed and the save succeeds
- THEN the menu closes, the file holds `enabled = false` under `[ai.tools]` and tools are off

#### Scenario: Popup size and fallback [auto, TestBackend + insta]
- GIVEN a terminal at exactly the minimum (24 columns by 15 rows), and one column or one row less
- WHEN the Tools tab and the picker are rendered
- THEN the first shows the full popup (clipped, no panic) and the second shows the one-line fallback

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN `Tools` exists with key path `ai.tools.enabled` and values `off`, `on`, and no key path is duplicated

#### Scenario: Tools row in Kitty [manual, Kitty]
- GIVEN the menu in Kitty
- WHEN `Tools` is turned off and saved, and a chat turn is sent
- THEN no tool row, no run hint and no confirm ever appears, and turning it on again restores them

### Requirement: Family toggle rows
The Tools tab MUST hold five static rows, one per family toggle (Search, Write, Exec, Git, GitWrite), placed after the `Tools` row and before `Approval`. Each reads `off` by default, with values `off`, `on` and the key path of its config key. Each is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter toggle the draft; an Enter repeat is ignored. Changing a draft MUST have no live effect and applies from the next turn after a successful save. Esc restores the draft; `s` saves only the keys that differ. The Write, Exec and GitWrite rows MUST show a one-line caution that the family can change files or run code, in the line reserved at the foot of the Tools tab. Navigation wraps inside the Tools tab, the popup does not change size, and the descriptor integrity rules extend to the new rows (no duplicate key path).
(Previously: rows of the AI tab)

#### Scenario: Navigation reaches every row [auto]
- GIVEN the Tools tab with `Tools` selected
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
- WHEN the Tools tab is rendered
- THEN the caution line is visible and the popup size is unchanged

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN the five rows exist with distinct key paths and values `off`, `on`

### Requirement: Approval row
The Tools tab MUST hold a static row `Approval` after the family rows and before `Orchestrator` (key path `ai.tools.approval`, values `ask`, `auto`, `run`; it reads `ask` by default). It is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter cycle the draft `ask`, `auto`, `run`, `ask` (an Enter repeat is ignored); the footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and `r` do not apply. Entering `run` needs no confirmation. Changing the draft MUST have no live effect: no relayout, no ResizePty or WritePty, no change to the mode of any open chat pane, a pending approval or a running tool; after a successful save it applies to chat panes opened afterwards and at the next conversation switch inside an open pane. Esc at the top level restores the draft; `s` saves `ai.tools.approval` through config-persistence only when it differs, and the value is written explicitly even when it equals the default. The row is shown whatever the `Tools` draft. The popup keeps its 16 rows and its 24x15 minimum (see Popup height is fixed); the descriptor integrity rules extend to the row.
(Previously: the last row of the AI tab; "With it the AI tab has 11 rows")

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
- GIVEN the Tools tab, a terminal of 80x24 and one of exactly 24x15
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

### Requirement: Orchestrate row and picker [S1]
Enter on the `orchestrate` row of the Models tab MUST open the same two-level task picker as the other tasks, with `(default · use general)` as its first entry. In its engine list the server engines MUST carry the status text plus `not recommended` and stay selectable. Choosing an entry changes only `orchestrate`. Enabling the orchestrator from the `Orchestrator` row of the Tools tab while `orchestrate` resolves to no engine MUST be allowed, and an orchestrating pane shows the engine guidance on the next send.
(Previously: both rows on the AI tab)

#### Scenario: Orchestrator row is the global gate [auto]
- GIVEN the Tools tab with `Orchestrator` `off`
- WHEN the row is cycled to `on` and saved
- THEN `[ai.orchestrator] enabled = true` is written, no pane changes mode by itself, and the next message of a pane with ORCH off is still a chat turn

#### Scenario: Picker first entry [auto]
- GIVEN the Models tab with `orchestrate` selected
- WHEN Enter is pressed
- THEN the title reads `Engine for orchestrate` and the first entry is `(default · use general)`

#### Scenario: Only orchestrate changes [auto]
- GIVEN `general` is `codex` and the picker for `orchestrate`
- WHEN `claude` then `opus` is chosen
- THEN `orchestrate` is `claude` / `opus` and the other tasks are unchanged

### Requirement: Picker opened from a failure notice [S2]
When the user picks `change model` or `change provider` in a worker failure alert, the app MUST open the menu task picker for that worker's kind (level 2 of the current engine for `change model`, level 1 for `change provider`), showing the same rows and marks as the menu. The picker starts from the effective assignment in use by the failed worker. A confirmed choice is a *run-scoped override*: it MUST apply until the run ends, MUST NOT modify the menu draft, MUST NOT be saved to the configuration, and MUST NOT make the menu show a pending change. It returns to the alert flow, which relaunches the worker on the override; later workers of that task in the same run use it too (see agent-orchestrator). The override MUST be shown in the run state and on the worker rows. Esc MUST create no override and return to the alert. While the picker is open the run stays paused.
(Previously: text unchanged; the last scenario said "the AI tab")

#### Scenario: Change model opens level 2 [auto]
- GIVEN a failed `review` worker on `claude`
- WHEN `change model` is chosen
- THEN the picker shows `Model for review (claude)` with the cursor on the current model

#### Scenario: Change provider opens level 1 [auto]
- GIVEN a failed `code` worker
- WHEN `change provider` is chosen
- THEN the picker shows `Engine for code`

#### Scenario: Choice is an override, not a save [auto]
- GIVEN the picker opened from a `review` alert and `claude` / `sonnet` confirmed
- WHEN the worker is relaunched
- THEN it runs on claude / `sonnet`, the menu draft and the saved file keep the earlier `review` assignment and no save is pending

#### Scenario: Menu value unchanged after the run [auto]
- GIVEN an override created in a run and the run ended
- WHEN the menu is opened and the Models tab shown
- THEN the `review` row shows the menu assignment from before the run

#### Scenario: Esc creates no override [auto]
- GIVEN the picker opened from an alert
- WHEN Esc is pressed
- THEN no override exists, the alert is shown and no worker is relaunched

### Requirement: Hugging Face account row
The Models tab MUST end with a static row `Hugging Face`, after the `orchestrate` row. Its value is:
- `not connected` when no token is found;
- `token saved` when a token file exists but no account has been checked yet; Enter on the row then checks it with that file's token (nothing is sent when the menu opens);
- `<name>` once an account is known, from a connect or a check in this session;
- `token set (HF_TOKEN)` while `HF_TOKEN` is set.

Enter on the row opens a field with the hint `paste a Hugging Face token`, followed by `create a read token at:` and, on a line of its own, `https://huggingface.co/settings/tokens/new?tokenType=read`. The field draws one `•` per character. Ctrl+O opens that link in the browser through the host's opener (`xdg-open`; on WSL `wslview`, then `rundll32.exe url.dll,FileProtocolHandler`; on macOS `open`), and the footer says `Opened in your browser`; on a host with no opener Ctrl+O copies the link instead. When the opener fails (it exits with an error within 5 seconds of its start; a later exit is the browser's own; the exit of a Windows program such as `rundll32.exe` is not checked), the link is copied, the footer says `The browser did not open: link copied`, and Ctrl+O copies from then on. With `Mouse` on, a left click on the link line does the same as Ctrl+O; any other click in the menu does nothing. With `Mouse` off the host terminal handles clicks itself. The field's footer is `Enter connect · Ctrl+O open link · Ctrl+U clear · Esc back`, or `… Ctrl+O copy link …` with no opener. Enter checks the token, with the footer `asking Hugging Face…`, and Esc goes back; Esc during the check stops it, and the token is not saved unless the save had already happened. On a known account, Enter offers instead `disconnect <name>? (y/n)`, which deletes the token file on `y`. With a saved token, `d` offers the same prompt, also when its account is unknown (`disconnect the saved token? (y/n)`), and the footer of such a row is `Enter check · d disconnect · j/k move · s save · Esc cancel`. The row writes no config edit.
(Previously: the last row of the AI tab, after `Approval`)

#### Scenario: Account row [auto]
- GIVEN the Models tab
- WHEN rendered with no token
- THEN the last row reads `Hugging Face` with `not connected`, after `orchestrate`

#### Scenario: Ctrl+O opens the token link [auto]
- GIVEN the token field, on a host with an opener
- WHEN Ctrl+O is pressed
- THEN `https://huggingface.co/settings/tokens/new?tokenType=read` is opened in the browser, the field stays open and the footer reads `Opened in your browser`
- AND with no opener the link is copied instead

#### Scenario: A click opens the token link [auto]
- GIVEN the token field with `Mouse` on, on a host with an opener
- WHEN the line that shows `https://huggingface.co/settings/tokens/new?tokenType=read` is clicked with the left button
- THEN the link is opened in the browser, the field stays open and the footer reads `Opened in your browser`
- AND a click on any other row of the field or of the menu does nothing

#### Scenario: The field hides the token [auto]
- GIVEN the token field
- WHEN `hf_abc` is pasted
- THEN the field shows `••••••` and no rendered cell holds `hf_abc`

## ADDED Requirements

### Requirement: Save output is unchanged by the tab split
Moving rows between tabs MUST NOT change what `s` writes. For the same set of changed draft values, the ordered list of config edits (keys, values and order) MUST be identical to the list produced before the split, whatever the order in which the user visited the tabs and changed the rows. A characterization test MUST pin that list before any refactor of how draft rows are declared, and the same test MUST pass after it. The keys, their values and the write order in `config.toml` do not change, and no migration is involved.

#### Scenario: Same edits as before the split [auto]
- GIVEN a golden list of edits captured before the split, from a draft that changes one row of every kind (Settings choices, Mouse, every task, Profile, Context, Memory, Notify, Sound, `llama-server idle`, Tools, the five family rows, Approval, Orchestrator)
- WHEN the same changes are made and the edits are read
- THEN the list equals the golden list in keys, values and order

#### Scenario: Visit order does not matter [auto]
- GIVEN the same set of changes made once Settings first then Chat and Models, and once in the reverse order
- WHEN the edits are read in both runs
- THEN the two lists are identical

#### Scenario: Unchanged tabs write nothing [auto]
- GIVEN every tab visited and nothing changed
- WHEN `s` is pressed
- THEN the menu closes and the file is not read or written

### Requirement: Usage row belongs to Settings
When the ai-cost `Usage (alpha)` row is added, it MUST be declared on the Settings tab, MUST NOT need a new tab or a change to navigation or drawing code, and MUST keep every other row on its tab. The popup grows by the one row that Settings then needs (see Popup height is fixed).

#### Scenario: Usage is on Settings [auto]
- GIVEN the ai-cost row declared in the Settings table
- WHEN the tab table is validated and the Settings tab is rendered
- THEN `Usage (alpha)` appears on Settings only, the tab list is unchanged, and the popup is 17 rows high on every tab

### Requirement: Docs follow the tabs
The documents that tell the user how to reach the AI settings MUST name the tabs and the jump keys: the agent-orchestrator `manual-tests.md` (T02 and every step that says to press Tab to reach AI), `docs/assets/tapes/ai-menu.tape`, the README menu section and the CHANGELOG entry (which MUST say that the `AI` tab was split and that `1`-`5` jump).

#### Scenario: Manual tests name the tab [manual]
- GIVEN `manual-tests.md` of agent-orchestrator
- WHEN T02 and the AI-tab steps are read
- THEN each step names the tab (Models, Tools or Chat) or the digit to reach it, and none says "Tab to AI"

#### Scenario: Tape reaches Models [manual]
- GIVEN `docs/assets/tapes/ai-menu.tape`
- WHEN it is run
- THEN it reaches the Models tab (one Tab or `2`) and the recording shows the new bar

#### Scenario: README and CHANGELOG [manual]
- GIVEN the README menu section and the CHANGELOG
- WHEN they are read
- THEN the README lists the five tabs, `Tab`/`Shift+Tab` and `1`-`5`, and the CHANGELOG notes the split

#### Scenario: Narrow terminals in Kitty [manual, Kitty]
- GIVEN the menu in Kitty resized to 48 and 49 columns with the AI section
- WHEN the bar is read and Tab is pressed
- THEN no panic occurs and the bar shows only the active tab, then the whole bar, as specified
