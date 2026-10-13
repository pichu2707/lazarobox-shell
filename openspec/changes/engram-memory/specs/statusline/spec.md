# Delta for Statusline

Tags: [auto] = `cargo test`; scenarios without a tag are [auto]. Existing snapshots, including those with the thinking indicator and the chat segments, MUST remain valid when memory is off.

## ADDED Requirements

### Requirement: Memory segment
While the focused pane is a chat pane and memory is enabled (see chat-memory), the right-hand segment MUST append ` · mem: <state>` after the engine text, where state is `ready` (drawn in `text_muted`), `checking` while a health probe or project resolution is in flight, or `unavailable` (drawn in `warning_orange`). With memory off, or when the focused pane is a shell pane, nothing is appended. When the width is not enough, this segment MUST be dropped as a whole before the engine text is shortened and before any part of the thinking indicator is dropped, and a part is never shown partially. The mode block keeps priority at every width and nothing panics.

#### Scenario: Ready [auto, TestBackend + insta]
- GIVEN a focused chat pane assigned `ollama` / `llama3:8b` with memory ready
- WHEN rendered on a wide terminal
- THEN the right side reads `ollama › llama3:8b · mem: ready`

#### Scenario: Unavailable and checking [auto]
- GIVEN memory enabled and the health probe in flight, then failed
- WHEN rendered each time
- THEN the right side ends with `mem: checking`, then `mem: unavailable` in `warning_orange`

#### Scenario: Memory off appends nothing [auto, TestBackend + insta]
- GIVEN memory off and a focused chat pane
- WHEN rendered
- THEN the statusline equals the one rendered without this requirement

#### Scenario: Shell pane focused [auto]
- GIVEN memory ready and a shell pane focused
- WHEN rendered
- THEN the right side shows the shell name and no memory text

#### Scenario: Dropped first when narrow [auto]
- GIVEN a streaming focused chat pane with memory ready and widths from 0 to 100
- WHEN rendered at each width
- THEN nothing panics, the memory segment disappears before the engine text is shortened and before any indicator part is dropped, and no part is cut in the middle
