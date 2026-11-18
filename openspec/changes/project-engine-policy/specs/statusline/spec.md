# Statusline Specification (delta: project engine policy)

## Base of this delta

This delta targets `openspec/specs/statusline/spec.md` after the change `local-only-mode` is archived (it adds "Local-only segment"). The MODIFIED block is a full copy of that requirement where only the project form of the prefix and one scenario are added. Slice tag: [P1].

## MODIFIED Requirements

### Requirement: Local-only segment
While local-only mode is on (see local-only-mode), the right-hand segment MUST start with `local only · ` in every pane: before the engine text of a chat pane (`local only · ollama › llama3:8b`, `local only · no engine`) and before the shell name of a shell pane (`local only · zsh`). While it is off and the focused pane is a chat whose rule requires local because of a project source (see project-engine-policy), the segment MUST start with `local only (project) · ` (`local only (project) · ollama › llama3:8b`); a shell pane gets no project prefix. While neither holds nothing is added and the statusline is byte-identical to the one rendered without this requirement. When the width is not enough, the memory and usage segments MUST be dropped first, then the engine or shell text is shortened, and the prefix is dropped last, as a whole; no part is shown partially. The prefix follows the saved value: a menu draft does not change it before the save. The mode block keeps priority at every width and nothing panics.

#### Scenario: Chat and shell panes [auto, TestBackend + insta]
- GIVEN local-only mode on, a focused chat pane on `ollama` / `llama3:8b`, then a focused zsh pane
- WHEN each is rendered on a wide terminal
- THEN the right side reads `local only · ollama › llama3:8b`, then `local only · zsh`

#### Scenario: Off adds nothing [auto, TestBackend + insta]
- GIVEN local-only mode off and no project rule
- WHEN a chat pane and a shell pane are rendered
- THEN the statusline equals the one rendered without this requirement

#### Scenario: Narrow [auto]
- GIVEN local-only mode on, memory ready, a usage line and widths from 0 to 120
- WHEN rendered at each width
- THEN nothing panics, memory and usage go before the engine text is shortened, the prefix goes last and whole, and no part is cut in the middle

#### Scenario: Project prefix [auto, TestBackend + insta]
- GIVEN local-only mode off, a focused chat on `ollama` / `llama3:8b` in a folder covered by a project entry with `local_only = true`, then a focused zsh pane in the same folder
- WHEN each is rendered on a wide terminal
- THEN the right side reads `local only (project) · ollama › llama3:8b`, then the shell text with no prefix
