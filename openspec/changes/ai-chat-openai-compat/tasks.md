# Tasks: AI Chat OpenAI-compatible adapter (moved from ai-chat PR14)

Moved from `openspec/changes/ai-chat/tasks.md`, section "PR14 / C12". Task text is verbatim; the checkboxes start open because the work has not been done.

## PR14 / C12: OpenAI-compatible (`adapters.rs`, `runtime/chat.rs`)

- [x] 14.1 RED: lmstudio, llamacpp and vllm bodies and paths (`/v1/chat/completions`), SSE decoder deltas, finish marker and `[DONE]` exactly one final, traffic audit per catalog port.
- [x] 14.2 RED (reasoning, if available): only if a captured fixture of lmstudio, llama.cpp or vLLM shows a separate reasoning field in the SSE delta, map it to `ThinkingDelta` (with split points and an unchanged request body); otherwise record "no reasoning exposed" in the PR body and add no task. No format is assumed.
- [x] 14.3 GREEN: implement; `opencode` still unsupported (adapter table scenario: seven adapters, none for opencode).
- [ ] 14.4 [manual] M7: a real lmstudio, llama.cpp or vLLM streams, the `[DONE]` ends the turn, cancel stops it.
- [ ] 14.5 Gate.

## Notes

- 14.1: recorded fixture `tests/fixtures/llamacpp/qwen2.5-plain.sse` (llama-server b11815, qwen2.5-0.5b-instruct q4_k_m, de-chunked, model path made neutral). No lmstudio or vLLM capture yet; they share the OpenAI SSE format and the same tests.
- 14.2: no reasoning exposed in the captured llama.cpp fixture (non-reasoning model), so no reasoning task was added and the body is unchanged.
- llama-server ignores the requested model name and streamed 200 for an unknown model, empty `messages` and an unknown role; no SSE `error` object was observed, so that path is covered by the synthetic decoder tests only.
- The picker change (engines without an adapter listed but disabled, footer reason) is specified in `specs/config-menu/spec.md`.
