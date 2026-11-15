# Delta for Configuration

## Base of this delta

This delta targets `openspec/specs/configuration/spec.md` (the "Simple slot [W8]" requirement is the model). Nothing existing is modified; one requirement is added. Slice [S3].

## ADDED Requirements

### Requirement: Judge slot [S3]
The configuration MUST carry an eighth AI task slot, `judge`, with the same `engine` and `model` keys and the same validation as the other tasks, read from the table `[ai.tasks.judge]`. An absent table means unassigned. Unlike `simple` and `orchestrate`, the slot MUST be counted by Set all and its aggregate (see ai-task-models). A build that does not know the table MUST ignore it and load every earlier key unchanged (the design verifies this for the previous schema before S3 ships). The menu writes the table only when the user assigns `judge` or uses Set all, and a file with no `judge` table MUST gain none from a save that did not touch it, byte for byte as for the other slots.

#### Scenario: Slot parsed [S3, auto]
- GIVEN `[ai.tasks.judge]` with `engine = "claude"` and `model = "opus"`
- WHEN the configuration is loaded
- THEN `judge` is `claude` / `opus` and the other slots are as before

#### Scenario: Absent means unassigned [S3, auto]
- GIVEN no `[ai.tasks.judge]` table
- WHEN the configuration is loaded
- THEN `judge` is unassigned and falls back to `general`

#### Scenario: Older build ignores the table [S3, auto]
- GIVEN a file with `[ai.tasks.judge]` read by the previous schema
- WHEN it is loaded
- THEN the earlier keys load unchanged and the new table is ignored, with no startup notice about it

#### Scenario: Invalid value follows the task rules [S3, auto]
- GIVEN `[ai.tasks.judge]` with a wrong type for `engine`
- WHEN the configuration is loaded
- THEN the slot falls back as the other task slots do and a startup notice names the key

#### Scenario: Set all writes judge [S3, auto]
- GIVEN no `judge` table and `codex` / `default` chosen for Set all tasks
- WHEN the menu saves
- THEN `[ai.tasks.judge]` holds `codex` / `default` together with the other five Set-all tasks

#### Scenario: Untouched file gains no table [S3, auto]
- GIVEN a file with no `judge` table and a save that changes only `Mouse`
- WHEN the file is written
- THEN no `[ai.tasks.judge]` table appears
