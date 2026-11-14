# Chat Engines Specification (delta: local-only mode)

## Base of this delta

This delta targets `openspec/specs/chat-engines/spec.md` on e480992e. The MODIFIED block is a full copy of "Engine resolution for chat" where only the list of results, one sentence on their order and two scenarios change. "Availability by task" already makes the same result hold for every task. Slice tags: [S1] (cloud CLIs), [S3] (Ollama cloud models).

## MODIFIED Requirements

### Requirement: Engine resolution for chat
The engine of a chat turn MUST come from the effective assignment of the `chat` task (its own assignment, else `general`; see ai-task-models) combined with the detection result. The result MUST be exactly one of: ready (engine and model), unassigned, refused by local only (a cloud engine, or from S3 an `ollama` assignment whose model is set and is a cloud model, while local-only mode is on; see local-only-mode), undetected (with the status), unsupported, no jail (`codex` or `gemini` on a system without a working jail, see CLI jail), gated off (a CLI whose manual gate failed), or model problem (no model for a server, or a model that is not detected). Refused by local only is checked right after unassigned and before every other result, so it wins over unsupported, no jail and gated off. Resolution MUST be pure, MUST be evaluated on every send, and MUST NEVER change the assignment or pick another engine.

#### Scenario: Ready server [auto]
- GIVEN `chat` is `ollama` / `llama3:8b`, ollama is `available` and lists `llama3:8b`
- WHEN resolved
- THEN the result is ready with that engine and model

#### Scenario: Falls back to general [auto]
- GIVEN `chat` is unassigned and `general` is `claude` / `sonnet`, claude `available`
- WHEN resolved
- THEN the result is ready with `claude` / `sonnet`

#### Scenario: Each guidance result [auto]
- GIVEN (a) nothing assigned, (b) `chat` ollama and ollama `not installed`, (c) `chat` opencode `available`, (d) `chat` codex available but gated off, (e) `chat` ollama with an empty model, (f) `chat` ollama / `x` not listed, (g) `chat` codex available on a system whose jail probe is unavailable
- WHEN each is resolved
- THEN they are unassigned, undetected, unsupported, gated off, model problem, model problem, no jail respectively

#### Scenario: Never switches [auto]
- GIVEN `chat` is `ollama` / `llama3:8b`, ollama is `not running` and claude is `available`
- WHEN resolved
- THEN the result is undetected for ollama and the assignment of `chat` is unchanged

#### Scenario: Ollama latest equivalence [auto]
- GIVEN `chat` is `ollama` / `llama3` and ollama lists `llama3:latest`
- WHEN resolved
- THEN the result is ready

#### Scenario: Re-resolved on every send [auto]
- GIVEN a ready chat and then the menu saves a different `chat` engine
- WHEN the next message is sent
- THEN the new engine is used for that message and the old one is not contacted

#### Scenario: Refused by local only [auto]
- GIVEN local-only mode on and, one at a time, `chat` `claude` / `sonnet` (available), `chat` `opencode` (available, no adapter), and `chat` `codex` available on a system whose jail probe is unavailable
- WHEN each is resolved
- THEN each is refused by local only naming its engine, not ready, unsupported or no jail, and the assignment is unchanged

#### Scenario: Local only leaves local engines alone [auto]
- GIVEN local-only mode on and `chat` `ollama` / `llama3:8b`, available and listed
- WHEN resolved
- THEN the result is ready with that engine and model
