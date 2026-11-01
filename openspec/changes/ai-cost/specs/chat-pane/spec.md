# Delta for Chat Pane

Tags as in the main spec. Usage semantics and text formats live in `ai-usage`.

## ADDED Requirements

### Requirement: The `/cost` command
`/cost` MUST be a user-only composer command, recognized when the composer text, trimmed of surrounding whitespace, is exactly `/cost` (case-sensitive). Sending it MUST NOT create a user message, MUST NOT emit ChatSend, MUST NOT start a stream, MUST NOT contact any engine, MUST NOT be stored in the conversation and MUST NOT enter the history of later requests. It clears the composer and appends the `/cost` view (see `ai-usage`) as system lines. It MUST work while a stream is running in the pane (the send-refusal rule does not apply) and in a conversation with no messages. `/cost` followed by anything (`/cost now`) MUST show the system line `/cost takes no arguments` and keep the composer text. The view's lines are system lines: not stored, not sent to an engine, not copied as part of an answer. If the pane has no engine configured, `/cost` still works.

#### Scenario: No message, no request [auto]
- GIVEN an idle chat and the composer holding `/cost`
- WHEN send is requested
- THEN no user message is added, no ChatSend effect is emitted, the composer is empty and the view lines are appended

#### Scenario: Works during a stream [auto]
- GIVEN a stream running and the composer holding `/cost`
- WHEN send is requested
- THEN the view is appended, the stream is unaffected and no wait-or-cancel line appears

#### Scenario: Not in the history [auto]
- GIVEN `/cost` was run and then `hello` is sent
- WHEN the request is built
- THEN the history holds `hello` only and nothing from `/cost`

#### Scenario: Not stored [auto]
- GIVEN a conversation where `/cost` was run
- WHEN the store is read
- THEN it holds no message with `/cost` or the view text

#### Scenario: Arguments refused [auto]
- GIVEN the composer holding `/cost now`
- WHEN send is requested
- THEN the system line `/cost takes no arguments` appears and the composer still holds `/cost now`

#### Scenario: Other text is a normal message [auto]
- GIVEN the composer holding `what does /cost do`
- WHEN send is requested
- THEN it is sent as a normal user message

#### Scenario: Pending read [auto]
- GIVEN a slow store
- WHEN `/cost` is sent
- THEN `reading usage…` shows at once and the view replaces it when the data arrives, without blocking keys

### Requirement: Per-answer usage line
After a turn that ended with a final event, the pane MUST append one dim system line below the answer, in the format of `ai-usage` (Usage text formats), when the `ai.usage.line` setting is on (default on). For a turn with no record the line reads `usage: no data`. For a cancelled, interrupted or errored turn the line appears only when the turn has a record (it is then marked `partial`). The line is not part of the assistant message: not stored, not sent to an engine, not included by yank of an answer, and not shown after resume. With the setting off, no usage line appears and nothing else changes. The line appears before any threshold note of the same turn.

#### Scenario: Line after a claude turn [auto, TestBackend + insta]
- GIVEN a claude turn with 2 in, 4 out, cost 0.004936 and mode `subscription`
- WHEN the turn ends
- THEN the line `usage: in 2 · out 4 · $0.0049 list-price equivalent` appears below the answer

#### Scenario: Local engine [auto]
- GIVEN an ollama turn with 12 in and 34 out
- WHEN the turn ends
- THEN the line reads `usage: in 12 · out 34`

#### Scenario: No data [auto]
- GIVEN a turn whose engine reported nothing
- WHEN the turn ends
- THEN the line reads `usage: no data`

#### Scenario: Turned off [auto]
- GIVEN `ai.usage.line` off
- WHEN a turn ends
- THEN no usage line appears

#### Scenario: Cancelled without usage [auto]
- GIVEN a turn cancelled before any usage was reported
- WHEN the pane returns to idle
- THEN no usage line appears

#### Scenario: Not stored or resumed [auto]
- GIVEN a turn with a usage line
- WHEN the store is read and the conversation is resumed
- THEN neither holds the line

#### Scenario: Yank excludes it [auto]
- GIVEN an answer followed by its usage line
- WHEN the answer is yanked
- THEN the yanked text does not contain the usage line

### Requirement: Threshold notes in the pane
Threshold notes (see `ai-usage`, Thresholds) MUST appear as dim system lines after the usage line of the turn that crossed them, in this order when several apply: turn, daily tokens, daily dollars. They MUST NOT block the composer, delay a send, hide the answer or change focus, and MUST NOT play a sound or notification of their own. They are shown whether or not the usage line is on.

#### Scenario: Note after the usage line [auto]
- GIVEN `turn_tokens` 100 and a turn of 300 tokens with the usage line on
- WHEN the turn ends
- THEN the usage line is followed by `turn used 300 tokens (limit 100)`

#### Scenario: Usage line off, note on [auto]
- GIVEN the usage line off and a crossed threshold
- WHEN the turn ends
- THEN only the note appears

#### Scenario: Composer stays usable [auto]
- GIVEN a note was shown
- WHEN a message is typed and sent
- THEN it is sent normally
