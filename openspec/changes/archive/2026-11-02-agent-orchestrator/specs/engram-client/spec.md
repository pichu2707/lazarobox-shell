# Delta for engram-client

Slice tags as in agent-orchestrator.

## ADDED Requirements

### Requirement: Topic-key upsert [S4]
Saving a note with a topic key that already exists in the same project MUST update that note and not create a second one, so a retried worker or re-closed ODD step leaves one handoff. The call stays bounded and off the render thread like other calls, and errors carry no content.

#### Scenario: Second save updates [auto]
- GIVEN a fake engram holding a note with key K
- WHEN a save with key K is sent
- THEN the fake receives an upsert and holds one note with the new content

#### Scenario: Failure is bounded [auto]
- GIVEN a fake engram that never replies
- WHEN a handoff save is sent with paused time advanced
- THEN the call ends at its bound with an error that holds no note text
