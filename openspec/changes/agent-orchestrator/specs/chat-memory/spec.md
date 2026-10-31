# Delta for chat-memory

Slice tags as in agent-orchestrator.

## ADDED Requirements

### Requirement: Handoff notes respect opt-in and redaction [S4]
Handoff notes (see orchestrator-handoffs) MUST be written only when chat memory is on, or when an orchestrator run turned it on for its own handoffs (only if the user accepted the privacy note earlier; see orchestrator-handoffs), MUST pass the same redaction pass as other notes, and MUST NOT be written, queued for later or sent anywhere otherwise. A run turning memory on MUST NOT make other chat notes be written. A handoff note MUST NOT appear as a user note in the memory view without its handoff category, and MUST be removed with its conversation under the same deletion rules only if the run belongs to that conversation.

#### Scenario: Off writes nothing [auto]
- GIVEN memory off and no orchestrator run
- WHEN a note is produced
- THEN no note, request or queue entry exists

#### Scenario: Never acknowledged: not turned on [auto]
- GIVEN memory never acknowledged and an orchestrator run
- WHEN a worker ends
- THEN no memory command is sent and no note is written

#### Scenario: Redacted [auto]
- GIVEN memory on and a handoff with a secret-like string
- WHEN it is saved
- THEN the stored note holds the redacted text

#### Scenario: Categorized in the view [auto]
- GIVEN a handoff note
- WHEN the memory view lists notes
- THEN it shows with its handoff category
