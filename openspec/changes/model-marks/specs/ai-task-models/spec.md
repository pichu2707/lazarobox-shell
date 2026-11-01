# Delta for ai-task-models

## ADDED Requirements

### Requirement: Tool marks on installed Ollama models
In the Ollama model list, each detected model (the `✓` rows) MUST carry a tool mark next to `✓`, decided only by its recorded size and capability (see ai-engine-detection, Ollama model size and tool capability):
- capability `tools` and size 3B or more: the mark `🔧`, so the row reads `<name> ✓ 🔧`;
- capability `tools` and size under 3B: the mark `tools: unreliable`, so the row reads `<name> ✓ tools: unreliable`;
- capability `no tools`, capability `unknown`, or size `unknown`: no tool mark. Unknown never produces a mark.

Exactly 3B counts as 3B or more. The mark describes the model, not the configuration: it MUST show the same when `[ai.tools] enabled = false`. The mark is display only: it MUST NOT change selectability, the assignment, the saved value or the task row text. Rows not detected (`download required`, curated catalog) get no tool mark. A mark too long for the row is clipped by display width and never wraps or misaligns the row.

#### Scenario: Tools and large enough [auto]
- GIVEN ollama detects `llama3:8b` with capability `tools` and size 8B
- WHEN the list is built
- THEN the row reads `llama3:8b ✓ 🔧`

#### Scenario: Tools but small [auto]
- GIVEN ollama detects `tiny:1b` with capability `tools` and size 1B
- WHEN the list is built
- THEN the row reads `tiny:1b ✓ tools: unreliable`

#### Scenario: Exactly 3B [auto]
- GIVEN ollama detects `mid:3b` with capability `tools` and size 3B
- WHEN the list is built
- THEN the row reads `mid:3b ✓ 🔧`

#### Scenario: No tools declared [auto]
- GIVEN ollama detects `gemma:7b` with capability `no tools`
- WHEN the list is built
- THEN the row reads `gemma:7b ✓` with no tool mark

#### Scenario: Unknown capability or size [auto]
- GIVEN ollama detects `a:7b` with capability `unknown`, and `b:latest` with capability `tools` and size `unknown`
- WHEN the list is built
- THEN both rows read `✓` with no tool mark

#### Scenario: Tools setting off [auto]
- GIVEN `[ai.tools] enabled = false` and ollama detects `llama3:8b` with capability `tools` and size 8B
- WHEN the list is built
- THEN the row still reads `llama3:8b ✓ 🔧`

#### Scenario: Mark does not change behaviour [auto]
- GIVEN a marked row
- WHEN Enter is pressed on it
- THEN the model is assigned exactly as for an unmarked `✓` row

#### Scenario: Not detected rows [auto]
- GIVEN code is assigned `ollama` / `old:7b` (not detected) and the curated catalog holds `mistral:7b`
- WHEN the list is built
- THEN `old:7b` and `mistral:7b` carry only `download required` and no tool mark

### Requirement: Tool mark on CLI engine rows
In the engine list of a picker, the engine rows `claude`, `codex` and `gemini` MUST carry the tool mark `🔧`, because lazarobox gives those engines its tools (the engines in the chat tool gate, `MCP_VERIFIED`). The mark follows that gate, not the configuration: it MUST show whether `[ai.tools] enabled` is true or false. It is shown together with the status text, which stays unchanged and first, with the mark after it. The `ollama` engine row MUST carry no tool mark, because only its models carry marks. The rows of `opencode`, `lmstudio`, `llamacpp` and `vllm` carry no tool mark. The CLI model rows (`Opus 5.5`, `default`, `custom…` and so on) MUST carry no tool mark. An engine listed as `(not detected)` keeps its text and gets no tool mark.

#### Scenario: CLI engine rows [auto]
- GIVEN claude, codex, gemini, opencode and ollama are available
- WHEN the engine list is built
- THEN the `claude`, `codex` and `gemini` rows carry `🔧`, and the `opencode` and `ollama` rows carry no tool mark

#### Scenario: Shown with tools off [auto]
- GIVEN `[ai.tools] enabled = false` and claude is available
- WHEN the engine list is built
- THEN the `claude` row still carries `🔧`

#### Scenario: CLI model rows unmarked [auto]
- GIVEN claude is available
- WHEN its model list is built
- THEN no row carries a tool mark

#### Scenario: Not detected engine [auto]
- GIVEN code is assigned `codex`, which is `not installed`
- WHEN the engine list is built
- THEN the row reads `codex (not detected)` with no tool mark

### Requirement: Tool mark text fallback
The tool mark MUST render as the emoji `🔧` only when the terminal can show it. When `TERM=linux`, or the locale is not UTF-8, every place that would show `🔧` MUST show the text `tools` instead. `tools: unreliable` is plain text and does not change. The choice MUST be made in one place, so every screen (engine rows, installed model rows, Ollama download screens) agrees. The fallback MUST NOT change selectability or alignment: rows stay within their width.

#### Scenario: Linux console [auto]
- GIVEN `TERM=linux` and a model with capability `tools` and size 8B
- WHEN the row is rendered
- THEN it reads `<name> ✓ tools`

#### Scenario: Non-UTF-8 locale [auto]
- GIVEN `TERM=xterm-256color` and a locale that is not UTF-8
- WHEN an engine row for `claude` is rendered
- THEN it carries `tools` and no `🔧`

#### Scenario: Normal terminal [auto]
- GIVEN `TERM=xterm-256color` and a UTF-8 locale
- WHEN the same rows are rendered
- THEN they carry `🔧`

#### Scenario: Real machine [manual]
- GIVEN a real Ollama with a tool model of 3B or more, a tool model under 3B and a model without tools
- WHEN the model picker is opened on a UTF-8 terminal and then on `TERM=linux`
- THEN the first shows `🔧`, the second `tools: unreliable`, the third no mark, and the console shows `tools` in place of `🔧`
