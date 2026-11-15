# Delta for Configuration

Slice tag: [W8].

## ADDED Requirements

### Requirement: Simple slot [W8]
The configuration MUST carry a seventh AI task slot, `simple`, with the same `engine` and `model` keys and the same validation as the other tasks, read from the table `[ai.tasks.simple]`. An absent table means unassigned. The slot MUST NOT be counted by any logic defined over the five original tasks (Set all, its aggregate). A build that does not know the table MUST ignore it and load every earlier key unchanged.

#### Scenario: Slot parsed [W8, auto]
- GIVEN `[ai.tasks.simple]` with `engine = "ollama"` and `model = "llama3:8b"`
- WHEN the configuration is loaded
- THEN `simple` is `ollama` / `llama3:8b` and the other slots are as before

#### Scenario: Absent means unassigned [W8, auto]
- GIVEN no `[ai.tasks.simple]` table
- WHEN the configuration is loaded
- THEN `simple` is unassigned and falls back to `general`

#### Scenario: Older build ignores the table [W8, auto]
- GIVEN a file with `[ai.tasks.simple]` read by the previous schema
- WHEN it is loaded
- THEN the earlier keys load unchanged and the new table is ignored

#### Scenario: Invalid value follows the task rules [W8, auto]
- GIVEN `[ai.tasks.simple]` with a wrong type for `engine`
- WHEN the configuration is loaded
- THEN the slot falls back as the other task slots do and a startup notice names the key

## Open points
- No new `[ai.orchestrator]` key is decided by the proposal (the 400-line limit, the two clarification rounds and the brief format are fixed values). Whether any of them becomes a key is not decided and is not specified here.
