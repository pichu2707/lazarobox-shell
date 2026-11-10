# Proposal: AI Chat OpenAI-compatible adapter (follow-up to ai-chat)

## Intent

Ship the deferred PR14 of `ai-chat`: the OpenAI-compatible adapter for `lmstudio`, `llamacpp` and `vllm`. `ai-chat` was archived with PR14 out of scope (user priority). Its verify report (engram `sdd/ai-chat/verify-report`, #3117) flagged that the spec then described adapters the code did not have; this change takes those requirements out of `ai-chat` so its archived spec matches the shipped code.

## Scope

In scope (tasks moved verbatim from `ai-chat` PR14, see `tasks.md`):
- Tasks 14.1 to 14.5: adapter bodies and `/v1/chat/completions` paths, SSE decoder, reasoning mapping only if a captured fixture exposes it, `opencode` still unsupported, manual check, gate.
- Requirements moved from `ai-chat` `chat-engines` (see `specs/chat-engines/spec.md`): the SSE decoder requirement, the OpenAI-compatible servers requirement, and the "Ollama and OpenAI-compatible servers" / real-engines scenarios.
- The adapter table in the ChatEngine contract changes from four adapters to seven, with `opencode` still without one.

Out of scope:
- Any change to Ollama or the CLI adapters.
- `opencode` (stays unsupported, guidance `not supported in chat yet`).

## Approach

Same loopback-only HTTP client as Ollama (`post_stream`), a new SSE decoder beside the NDJSON decoder, and one adapter per engine sharing the body builder. Reasoning is mapped only where a captured fixture of that engine shows a reasoning field; no format is assumed.

## Dependencies

- `ai-chat` archived (its `chat-engines` main spec is the base of this delta).
- Live servers for manual checks 14.4 (lmstudio, llama.cpp or vLLM).

## Risks

- No captured fixtures exist yet for the three servers; the body and SSE shapes must be recorded before GREEN (design step).
- Manual check 14.4 needs a real server; it stays open until run.
