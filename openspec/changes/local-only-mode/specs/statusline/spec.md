# Statusline Specification (delta: local-only mode)

## Base of this delta

This delta targets `openspec/specs/statusline/spec.md` on e480992e. It adds one segment; "Chat pane segments", "Memory segment" and "Usage segment" are unchanged when the mode is off, and with the mode on their text follows the new prefix. Slice tag: [S1].

## ADDED Requirements

### Requirement: Local-only segment
While local-only mode is on (see local-only-mode), the right-hand segment MUST start with `local only · ` in every pane: before the engine text of a chat pane (`local only · ollama › llama3:8b`, `local only · no engine`) and before the shell name of a shell pane (`local only · zsh`). While it is off nothing is added and the statusline is byte-identical to the one rendered without this requirement. When the width is not enough, the memory and usage segments MUST be dropped first, then the engine or shell text is shortened, and the prefix is dropped last, as a whole; no part is shown partially. The prefix follows the saved value: a menu draft does not change it before the save. The mode block keeps priority at every width and nothing panics.

#### Scenario: Chat and shell panes [auto, TestBackend + insta]
- GIVEN local-only mode on, a focused chat pane on `ollama` / `llama3:8b`, then a focused zsh pane
- WHEN each is rendered on a wide terminal
- THEN the right side reads `local only · ollama › llama3:8b`, then `local only · zsh`

#### Scenario: Off adds nothing [auto, TestBackend + insta]
- GIVEN local-only mode off
- WHEN a chat pane and a shell pane are rendered
- THEN the statusline equals the one rendered without this requirement

#### Scenario: Narrow [auto]
- GIVEN local-only mode on, memory ready, a usage line and widths from 0 to 120
- WHEN rendered at each width
- THEN nothing panics, memory and usage go before the engine text is shortened, the prefix goes last and whole, and no part is cut in the middle
