# Delta for configuration

Slice tags as in agent-orchestrator.

## ADDED Requirements

### Requirement: Orchestrate slot [S1]
The configuration MUST carry a sixth AI task slot, `orchestrate`, with the same `engine` and `model` keys and the same validation as the other tasks, read from the table `[ai.tasks.orchestrate]`. An absent table means unassigned. The slot MUST NOT be counted by any logic that is defined over the five original tasks (Set all, its aggregate).

#### Scenario: Slot parsed [auto]
- GIVEN `[ai.tasks.orchestrate]` with `engine = "claude"` and `model = "opus"`
- WHEN the configuration is loaded
- THEN `orchestrate` is `claude` / `opus` and the other slots are as before

#### Scenario: Absent means unassigned [auto]
- GIVEN no `[ai.tasks.orchestrate]` table
- WHEN the configuration is loaded
- THEN `orchestrate` is unassigned and falls back to `general`

### Requirement: Orchestrator keys [S1, S3]
The configuration MUST read these keys under `[ai.orchestrator]`: `enabled` (bool, default `false`; the global gate), `notice_accepted` (bool, default `false`), `worker_timeout_secs` (integer, default 600, valid range 30 to 3600; the worker deadline), [S3] `max_parallel` (integer, default 2, valid range 1 to 4), `max_workers` (integer, default 8, valid range 1 to 32) and `run_timeout_mins` (integer, default 60, valid range 1 to 480). The per-pane ORCH mode is not a configuration key. A wrong type or out-of-range value MUST fall back to the default for that key and produce the existing startup notice, without discarding other keys. Unknown keys MUST be ignored.

#### Scenario: Defaults [auto]
- GIVEN no `[ai.orchestrator]` table
- WHEN the configuration is loaded
- THEN `enabled` and `notice_accepted` are false, `worker_timeout_secs` is 600, `max_parallel` is 2, `max_workers` is 8 and `run_timeout_mins` is 60

#### Scenario: Bad value falls back [auto]
- GIVEN `max_parallel = 0` and `enabled = true`
- WHEN the configuration is loaded
- THEN `max_parallel` is 2, `enabled` is true and a startup notice names the key

#### Scenario: Older build ignores keys [auto]
- GIVEN a file with `[ai.tasks.orchestrate]` and `[ai.orchestrator]` read by the previous schema
- WHEN it is loaded
- THEN the earlier keys load unchanged and the new ones are ignored
