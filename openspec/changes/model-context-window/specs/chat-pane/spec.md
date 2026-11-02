# Delta for Chat Pane

## MODIFIED Requirements

### Requirement: Context truncation marker
When the history sent to the engine omits older messages because of the client-side budget (the message cap, the character cap, or the real context window; see chat-engines), the transcript MUST show the system line `earlier messages are not sent to the model` once, at the position of the oldest message that is sent. The marker MUST NOT be stored.
(Previously: the marker covered only the message and character caps.)

#### Scenario: Marker shown [auto]
- GIVEN a conversation longer than the history cap
- WHEN a message is sent
- THEN the marker appears before the oldest sent message and is absent from the stored conversation

#### Scenario: Short conversation has no marker [auto]
- GIVEN a conversation within the cap
- WHEN a message is sent
- THEN no marker is shown

#### Scenario: Window trim shows the marker [auto]
- GIVEN a forced small window that drops the oldest pair
- WHEN a message is sent
- THEN the marker appears before the oldest sent message and is absent from the stored conversation
