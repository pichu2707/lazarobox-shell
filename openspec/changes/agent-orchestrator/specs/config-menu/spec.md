# Delta for config-menu

Slice tags as in agent-orchestrator.

## MODIFIED Requirements

### Requirement: AI task list [S1]
The AI tab MUST show, as its rows (with no section title row while the tab bar is shown), exactly seven task rows in this order: `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, and then the static rows `Profile`, `Context`, `Memory`, `Tools` and `Orchestrator`. Each task row shows its label and its value (see ai-task-models): `<engine> › <model>`, `(default)` for an unassigned non-general task, `(not set)` for an unset `general` or an unset aggregate, `(mixed)` for the aggregate when the five tasks it covers differ, and the marks `· download required`, `(not detected)` and, on `orchestrate` only, `· not recommended` as defined there. The `Profile` row shows the profile id; `Context`, `Memory`, `Tools` and `Orchestrator` show `off` or `on`; `Orchestrator` is the `[ai.orchestrator] enabled` switch (the global gate; default `off`; each chat pane then opts in with its own ORCH toggle, see chat-pane) and cycles like the other on/off rows. The number of AI rows MUST never change because of a detection result. The popup MUST show every row of the active tab without clipping on a normal terminal, MUST keep the same size between both tabs and the picker, and the minimum terminal size below which only the one-line fallback is drawn (see Tiny terminals) MUST follow the popup; the exact dimensions are a design decision.
(Previously: six task rows without `orchestrate`, and no `Orchestrator` row)

#### Scenario: Task rows rendered [auto, TestBackend + insta]
- GIVEN general is `claude` / `sonnet`, code is `ollama` / `llama3:8b` (ollama available with that model), the other tasks unassigned, the profile `analyst`, and the AI tab active
- WHEN the menu is rendered on a normal terminal
- THEN the rows are visible: `Set all tasks` `(mixed)`, `general` `claude › sonnet`, `code` `ollama › llama3:8b`, `chat` `(default)`, `review` `(default)`, `summarize` `(default)`, `orchestrate` `(default)`, `Profile` `analyst`, and the `Context`, `Memory`, `Tools` and `Orchestrator` rows with their `off` or `on` value

#### Scenario: Nothing assigned [auto, TestBackend + insta]
- GIVEN no task is assigned and the AI tab active
- WHEN the menu is rendered
- THEN `Set all tasks` reads `(not set)`, `general` reads `(not set)`, the other five read `(default)`, `Profile` reads `programmer` (the default profile) and `Context`, `Memory`, `Tools` and `Orchestrator` read their defaults (`Orchestrator` `off`)

#### Scenario: Marks on rows [auto]
- GIVEN code is `ollama` / `old:7b` not downloaded, and review is `codex` / `default` with codex `not installed`
- WHEN the AI tab is rendered
- THEN the rows read `ollama › old:7b · download required` and `codex (not detected) › default`

#### Scenario: Popup size is stable [auto, TestBackend + insta]
- GIVEN the Settings tab, the AI tab and then the picker open on a normal terminal
- WHEN each is rendered
- THEN the popup has the same size in all and every row of the active tab is visible

#### Scenario: Fallback threshold follows the popup [auto]
- GIVEN a terminal one column or one row smaller than the popup minimum, and one exactly at it
- WHEN rendered
- THEN the first shows the one-line fallback and the second shows the full popup without panic

## ADDED Requirements

### Requirement: Orchestrate row and picker [S1]
Enter on the `orchestrate` row MUST open the same two-level task picker as the other tasks, with `(default · use general)` as its first entry. In its engine list the server engines MUST carry the status text plus `not recommended` and stay selectable. Choosing an entry changes only `orchestrate`. Enabling the orchestrator from the `Orchestrator` row while `orchestrate` resolves to no engine MUST be allowed, and an orchestrating pane shows the engine guidance on the next send.

#### Scenario: Orchestrator row is the global gate [auto]
- GIVEN the AI tab with `Orchestrator` `off`
- WHEN the row is cycled to `on` and saved
- THEN `[ai.orchestrator] enabled = true` is written, no pane changes mode by itself, and the next message of a pane with ORCH off is still a chat turn

#### Scenario: Picker first entry [auto]
- GIVEN the AI tab with `orchestrate` selected
- WHEN Enter is pressed
- THEN the title reads `Engine for orchestrate` and the first entry is `(default · use general)`

#### Scenario: Only orchestrate changes [auto]
- GIVEN `general` is `codex` and the picker for `orchestrate`
- WHEN `claude` then `opus` is chosen
- THEN `orchestrate` is `claude` / `opus` and the other tasks are unchanged

### Requirement: Picker opened from a failure notice [S2]
When the user picks `change model` or `change provider` in a worker failure alert, the app MUST open the menu task picker for that worker's kind (level 2 of the current engine for `change model`, level 1 for `change provider`), showing the same rows and marks as the menu. The picker starts from the effective assignment in use by the failed worker. A confirmed choice is a *run-scoped override*: it MUST apply until the run ends, MUST NOT modify the menu draft, MUST NOT be saved to the configuration, and MUST NOT make the menu show a pending change. It returns to the alert flow, which relaunches the worker on the override; later workers of that task in the same run use it too (see agent-orchestrator). The override MUST be shown in the run state and on the worker rows. Esc MUST create no override and return to the alert. While the picker is open the run stays paused.

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
- WHEN the AI tab is opened
- THEN the `review` row shows the menu assignment from before the run

#### Scenario: Esc creates no override [auto]
- GIVEN the picker opened from an alert
- WHEN Esc is pressed
- THEN no override exists, the alert is shown and no worker is relaunched

### Requirement: Provider notice view [S1]
The one-time provider notice MUST be an informational view or inline notice with one accept key and a way to dismiss it; it MUST NOT stop the user from using the chat or the menu while it is shown, and dismissing without accepting MUST leave `notice_accepted` false.

#### Scenario: Accept persists [auto]
- GIVEN the notice is shown
- WHEN the accept key is pressed
- THEN `notice_accepted` is true and written to the config file at once (one config write, whose result never closes or marks a menu opened meanwhile), and the notice is gone; a failed write leaves it false and says why

#### Scenario: Dismiss does not accept [auto]
- GIVEN the notice is shown
- WHEN it is dismissed
- THEN `notice_accepted` stays false
