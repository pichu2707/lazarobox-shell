# Delta for Chat Pane

Tags: [auto] = `cargo test` (pure state machine, `TestBackend` + insta); [manual] = real machine. All existing requirements are unchanged; this delta only adds views, which exist only when memory is enabled (see chat-memory). With memory off the pane renders and behaves exactly as before.

## ADDED Requirements

### Requirement: Memory views in the pane
The chat pane MUST support three memory views, each drawn inside the pane area in place of the conversation view while it is open, like the conversation list and the tag editor: the note editor (category, title and content fields, the focused field marked, a one-line key hint), the summary editor (the content field alone, with the line `Summary (not saved until you approve)`), and the start-server confirmation (the offer text of engram-client with `(y/n)`). While one of them is open the composer text, the transcript, the scroll position and any running stream MUST be kept and keep updating underneath, and closing the view returns to the conversation view unchanged. A pane has at most one memory view at a time. All text drawn is already sanitized text in styled spans, and rendering MUST NOT panic at any pane size down to 1x1.

#### Scenario: Note editor drawn [auto, TestBackend + insta]
- GIVEN the note editor with category `decision`, title `Use Axum` and two lines of content in a 50x16 pane
- WHEN rendered
- THEN the three fields are visible with the focused one marked, and the key hint is on the last row

#### Scenario: Summary editor drawn [auto, TestBackend + insta]
- GIVEN the summary editor with a proposed summary
- WHEN rendered
- THEN the line `Summary (not saved until you approve)` and the content field are visible

#### Scenario: Stream continues under a view [auto]
- GIVEN a stream running and the note editor opened over the previous answer
- WHEN deltas and the final event arrive, then the editor is closed
- THEN the transcript holds the full new answer and the composer text is as before

#### Scenario: Composer survives the view [auto]
- GIVEN the composer holding `draft` and the note editor opened and discarded
- WHEN the conversation view is shown again
- THEN the composer still holds `draft`

#### Scenario: Tiny pane [auto]
- GIVEN a pane of any size from 1x1 to 10x2
- WHEN each memory view is rendered
- THEN nothing panics

#### Scenario: Memory off changes nothing [auto, TestBackend + insta]
- GIVEN memory off
- WHEN the existing chat pane snapshots are rendered
- THEN they equal those rendered without this requirement

### Requirement: Memory hint lines
Memory system lines (`memory unavailable: <reason>`, the server-started line, the notes-remain lines of chat-history, deletion results) MUST be system lines: visually distinct from messages, never stored, never sent to the model. The line `memory unavailable: <reason>` MUST be shown at most once per conversation view, and a system line MUST never repeat because of a repeated probe.

#### Scenario: Once per view [auto]
- GIVEN an unavailable memory and two failed memory-key probes in one conversation view
- WHEN the transcript is read
- THEN `memory unavailable: engram is not running` appears once

#### Scenario: Not stored, not sent [auto]
- GIVEN a conversation with the memory hint line
- WHEN a message is sent and the store commands and the engine history are inspected
- THEN neither contains the hint
