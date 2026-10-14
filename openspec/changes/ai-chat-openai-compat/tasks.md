# Tasks: AI Chat OpenAI-compatible adapter (moved from ai-chat PR14)

Moved from `openspec/changes/ai-chat/tasks.md`, section "PR14 / C12". Task text is verbatim; the checkboxes start open because the work has not been done.

## PR14 / C12: OpenAI-compatible (`adapters.rs`, `runtime/chat.rs`)

- [ ] 14.1 RED: lmstudio, llamacpp and vllm bodies and paths (`/v1/chat/completions`), SSE decoder deltas, finish marker and `[DONE]` exactly one final, traffic audit per catalog port.
- [ ] 14.2 RED (reasoning, if available): only if a captured fixture of lmstudio, llama.cpp or vLLM shows a separate reasoning field in the SSE delta, map it to `ThinkingDelta` (with split points and an unchanged request body); otherwise record "no reasoning exposed" in the PR body and add no task. No format is assumed.
- [ ] 14.3 GREEN: implement; `opencode` still unsupported (adapter table scenario: seven adapters, none for opencode).
- [ ] 14.4 [manual] M7: a real lmstudio, llama.cpp or vLLM streams, the `[DONE]` ends the turn, cancel stops it.
- [ ] 14.5 Gate.
