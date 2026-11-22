# Delta for Statusline (usage-dashboard)

## MODIFIED Requirements

### Requirement: Usage segment
While the focused pane is a chat pane and the `ai.usage.statusline` setting is on (default on), the right-hand segment MUST append ` · today: <T>` after the engine text and before the memory segment, where `<T>` is `<C> tok` with `<C>` the compact form of the sum of known total tokens of today's local-day usage records (`999`, `1.0k`, `123.5k`, `1.2M`, `11.6B`; `.` decimal point; see `usage-dashboard`, Number formatting), all chats and engines, including deleted chats, or `no data` when no record of the day has a known total. Everything else is unchanged: when the sum includes dollars of engines in `per-token` mode, ` · $<N>` is appended; dollars of `subscription` engines are never shown; the figures refresh when a usage record is stored and when the local day changes; until the first read the segment is absent (not `0`); with the setting off or a shell pane focused nothing is appended; width dropping order, mode-block priority and no-panic guarantees are unchanged.
(Previously: `<N> tok` with a plain integer, for example `today: 1234 tok`.)

#### Scenario: Shown with compact tokens [auto, TestBackend + insta]
- GIVEN a focused chat pane assigned `ollama` / `llama3:8b` and today's totals of 1234 tokens
- WHEN rendered on a wide terminal
- THEN the right side reads `ollama › llama3:8b · today: 1.2k tok`

#### Scenario: Small total [auto]
- GIVEN today's totals of 950 tokens
- WHEN rendered
- THEN the segment reads `today: 950 tok`

#### Scenario: Large total [auto]
- GIVEN today's totals of 1234567 tokens
- WHEN rendered
- THEN the segment reads `today: 1.2M tok`

#### Scenario: No data, not zero [auto]
- GIVEN today has records but none with a known total, and another run with no records at all after the first read
- WHEN rendered
- THEN the right side ends with `today: no data` both times

#### Scenario: Per-token dollars [auto]
- GIVEN claude in `per-token` mode with $0.42 today and 5000 tokens
- WHEN rendered
- THEN the right side contains `today: 5.0k tok · $0.42`

#### Scenario: Subscription dollars hidden [auto]
- GIVEN claude in `subscription` mode with $0.42 today
- WHEN rendered
- THEN the segment shows tokens and no `$`

#### Scenario: Before the memory segment [auto, TestBackend + insta]
- GIVEN memory ready and usage on
- WHEN rendered on a wide terminal
- THEN the right side reads `ollama › llama3:8b · today: 1.2k tok · mem: ready`

#### Scenario: Workers count [auto]
- GIVEN a displayed total of 1000 tokens
- WHEN a worker record of 300 tokens is stored
- THEN the next frame after the store answers shows `today: 1.3k tok`

#### Scenario: Refresh after a turn [auto]
- GIVEN a displayed total of 1000 tokens
- WHEN a turn of 200 tokens is stored
- THEN the next frame after the store answers shows `today: 1.2k tok`

#### Scenario: Unchanged behaviours [auto]
- GIVEN the scenarios `Before the first read`, `Off` and `Dropped first when narrow`
- WHEN run against the new format
- THEN they pass with only the number format changed
