# Delta for Configuration

## Base of this delta

This delta targets `openspec/specs/configuration/spec.md` (the "Simple slot [W8]" requirement is the model). Nothing existing is modified; one requirement is added. Slice [S3]. Decision J17: the judges are two slots, `judge-a` and `judge-b`, of the section `ODD + RDD roles` of issue #765.

## ADDED Requirements

### Requirement: Judge slots [S3]
The configuration MUST carry two more AI task slots, `judge-a` and `judge-b`, with the same `engine` and `model` keys and the same validation as the other tasks, read from the tables `[ai.tasks.judge-a]` and `[ai.tasks.judge-b]`. An absent table means unassigned. Unlike the five Set-all tasks, the slots are NOT counted by `Set all tasks` and its aggregate; they are covered by `Set all ODD + RDD roles` (see ai-task-models). A build that does not know a table MUST ignore it, MUST report it as an unknown key (`unknown key: ai.tasks.judge-a`) in the visible configuration problems like any other unknown key under `ai.tasks`, and MUST load every earlier key unchanged (the design verifies this for the previous schema before S3 ships). The menu writes a table only when the user assigns that judge or uses `Set all ODD + RDD roles`, and a file with no judge table MUST gain none from a save that did not touch it, byte for byte as for the other slots.

#### Scenario: Slots parsed [S3, auto]
- GIVEN `[ai.tasks.judge-a]` with `engine = "claude"` and `model = "opus"` and `[ai.tasks.judge-b]` with `engine = "codex"`
- WHEN the configuration is loaded
- THEN `judge-a` is `claude` / `opus`, `judge-b` is `codex` and the other slots are as before

#### Scenario: Absent means unassigned [S3, auto]
- GIVEN no judge table
- WHEN the configuration is loaded
- THEN both judges are unassigned and fall back as ai-task-models states

#### Scenario: Older build reports the table and keeps working [S3, auto]
- GIVEN a file with `[ai.tasks.judge-a]` read by the previous schema
- WHEN it is loaded
- THEN the earlier keys load unchanged, the table is ignored, and the visible configuration problems name `unknown key: ai.tasks.judge-a`

#### Scenario: Invalid value follows the task rules [S3, auto]
- GIVEN `[ai.tasks.judge-b]` with a wrong type for `engine`
- WHEN the configuration is loaded
- THEN the slot falls back as the other task slots do and a startup notice names the key

#### Scenario: Set all ODD + RDD roles writes the judges [S3, auto]
- GIVEN no judge table and `codex` / `default` chosen for `Set all ODD + RDD roles`
- WHEN the menu saves
- THEN `[ai.tasks.judge-a]` and `[ai.tasks.judge-b]` each hold `codex` / `default` and no other task table changes

#### Scenario: Set all tasks writes no judge table [S3, auto]
- GIVEN no judge table and `codex` / `default` chosen for Set all tasks
- WHEN the menu saves
- THEN no judge table appears

#### Scenario: Untouched file gains no table [S3, auto]
- GIVEN a file with no judge table and a save that changes only `Mouse`
- WHEN the file is written
- THEN no judge table appears
