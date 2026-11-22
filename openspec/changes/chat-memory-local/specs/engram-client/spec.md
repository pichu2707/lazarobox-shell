# Delta for Engram Client

## ADDED Requirements

### Requirement: Automatic mirror of local notes [S5]

The client MUST mirror local notes automatically only when engram is installed AND engram memory is enabled and acknowledged for the project (the existing opt-in and privacy warning). There is no per-note approval. Local `.lzr/` is the only source of truth: a note is written locally first, then mirrored through the existing `Save` command with `type` equal to the kind, `scope` `project`, `topic_key` `chat/<kind>/<slug(title)>`, and only redacted content. The mirror MUST obey the worker rules (bounded calls, 64-command queue, health cache, loopback only, no content in logs or errors). When engram is missing, disabled, not acknowledged or not a git project, the mirror MUST send nothing, start no worker, offer no server start for it, and show no error and no extra notice. When engram is down, the mirror is retried later (never in a timer loop: on the next save or memory action, at least 10 s apart), local memory is unaffected, and one visible line `memory: engram unavailable; local memory is not affected` is shown once per run. A mirrored delete or replacement MUST follow the user's confirmation rules of "Deleting notes"; the mirror never deletes on its own.

#### Scenario: Disabled means no traffic
- GIVEN engram running, memory not enabled, and a recording fake
- WHEN notes are saved
- THEN the fake records no connection

#### Scenario: Mirror after local write
- GIVEN engram memory enabled and acknowledged, and a fake
- WHEN a note is saved
- THEN `.lzr/` holds it first, then one `POST /observations` carries the topic key and redacted content

#### Scenario: Retried later
- GIVEN a fake that refuses the first save and answers later
- WHEN the next memory action runs 11 s later
- THEN the unsent note is mirrored once and no duplicate exists

#### Scenario: No error without engram
- GIVEN no `engram` on PATH
- WHEN a chat runs and notes are saved
- THEN no `memory unavailable` line, notice or offer appears for the mirror

#### Scenario: Never deletes on its own
- GIVEN a local cleanup that removes 2 notes
- WHEN the mirror runs
- THEN no `DELETE` is sent without an explicit user confirmation
