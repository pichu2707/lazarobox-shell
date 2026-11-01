# Delta for Statusline

Tags as in the main spec.

## ADDED Requirements

### Requirement: Usage segment
While the focused pane is a chat pane and the `ai.usage.statusline` setting is on (default on), the right-hand segment MUST append ` · today: <T>` after the engine text and before the memory segment, where `<T>` is `<N> tok` (the sum of known total tokens of today's local-day usage records, all chats and engines, including deleted chats) or `no data` when no record of the day has a known total. When the sum includes dollars of engines in `per-token` mode (see `ai-usage`), ` · $<N>` is appended. Dollars of `subscription` engines are never shown in the segment. The figures refresh when a usage record is stored and when the local day changes; they are read off the UI thread, and until the first read the segment is absent (not `0`). With the setting off, or when the focused pane is a shell pane, nothing is appended. Width: when space is short, the segment MUST be dropped as a whole before the memory segment, before the engine text is shortened and before any part of the thinking indicator is dropped; it is never shown partially. The mode block keeps priority at every width and nothing panics.

#### Scenario: Shown with tokens [auto, TestBackend + insta]
- GIVEN a focused chat pane assigned `ollama` / `llama3:8b` and today's totals of 1234 tokens
- WHEN rendered on a wide terminal
- THEN the right side reads `ollama › llama3:8b · today: 1234 tok`

#### Scenario: No data, not zero [auto]
- GIVEN today has records but none with a known total, and another run with no records at all after the first read
- WHEN rendered
- THEN the right side ends with `today: no data` both times

#### Scenario: Before the first read [auto]
- GIVEN a chat pane opened and the usage read not yet answered
- WHEN rendered
- THEN no usage text appears

#### Scenario: Per-token dollars [auto]
- GIVEN claude in `per-token` mode with $0.42 today and 5000 tokens
- WHEN rendered
- THEN the right side contains `today: 5000 tok · $0.42`

#### Scenario: Subscription dollars hidden [auto]
- GIVEN claude in `subscription` mode with $0.42 today
- WHEN rendered
- THEN the segment shows tokens and no `$`

#### Scenario: Before the memory segment [auto, TestBackend + insta]
- GIVEN memory ready and usage on
- WHEN rendered on a wide terminal
- THEN the right side reads `ollama › llama3:8b · today: 1234 tok · mem: ready`

#### Scenario: Off [auto]
- GIVEN `ai.usage.statusline` off, or a shell pane focused
- WHEN rendered
- THEN the statusline equals the one rendered without this requirement

#### Scenario: Dropped first when narrow [auto]
- GIVEN a streaming focused chat pane with memory ready, usage on and widths from 0 to 100
- WHEN rendered at each width
- THEN nothing panics, the usage segment disappears before the memory segment, before the engine text is shortened and before any indicator part is dropped, and no part is cut in the middle

#### Scenario: Refresh after a turn [auto]
- GIVEN a displayed total of 1000 tokens
- WHEN a turn of 200 tokens is stored
- THEN the next frame after the store answers shows 1200
