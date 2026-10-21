# Delta for config-persistence

Slice tags as in agent-orchestrator.

## ADDED Requirements

### Requirement: Orchestrator keys are owned [S1]
Saving MUST own and write the `[ai.orchestrate]` table (`engine`, `model`) and the keys `enabled`, `notice_accepted` and `worker_timeout_secs` of `[ai.orchestrator]` (and [S3] `max_parallel`, `max_workers` and `run_timeout_mins`; the per-pane ORCH mode and run overrides are never written), under the same rules as the other owned keys: atomic write, comments and unrelated keys preserved, nested-table edits, refusal on a file that cannot be parsed. A cleared `orchestrate` slot MUST remove its keys rather than write empty strings. Saving the notice acceptance MUST NOT require any other change to be pending.

#### Scenario: Save the slot [auto]
- GIVEN `orchestrate` set to `claude` / `opus` in the draft
- WHEN s is pressed
- THEN `[ai.orchestrate]` holds `engine = "claude"` and `model = "opus"` and other content, comments and unknown keys are preserved

#### Scenario: Clearing removes the keys [auto]
- GIVEN a file with `[ai.orchestrate]` and the draft clears the slot
- WHEN it is saved
- THEN the keys are removed and the rest of the file is unchanged

#### Scenario: Notice acceptance saved [auto]
- GIVEN `notice_accepted` true in the draft and no other change
- WHEN it is saved
- THEN the file holds `notice_accepted = true` under `[ai.orchestrator]`

#### Scenario: A failure picker choice is never persisted [S2, auto]
- GIVEN the picker from a failure alert chooses `review` = `claude` / `sonnet` as a run-scoped override
- WHEN the choice is confirmed and the run later ends
- THEN no write to the config file was made, `[ai.review]` is byte-identical to before, and the next save of the menu does not include the override
