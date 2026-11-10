# Proposal: Native tools for llama.cpp

## Intent

The `chat` engine can use tools (shell, files, MCP) only with Ollama among local servers; llamacpp, lmstudio and vllm get the tool-less body (`src/core/ai/chat/adapters.rs:203-214`). Users running local llama.cpp cannot let the model work with the shell or the code. Give llamacpp the same bounded tool loop as Ollama over the OpenAI wire, without touching Ollama's verified behaviour.

## Scope

### In Scope
- `OpenAiToolDecoder` over `SseParser`: accumulates `delta.tool_calls` per `index`, flushes on `finish_reason:"tool_calls"` or `[DONE]`, releases calls only after a clean end; reuses `tool_call()` validation and `MAX_CALLS`.
- `openai_round_body`: string `arguments`, `{role:"tool", tool_call_id, content}` (synthesized `call_{round}_{n}` when absent); shares system/history assembly with `ollama_round_body`. No `parallel_tool_calls`, no reasoning echo.
- One tool loop parameterized by a wire profile (path, body builder, decoder factory); Ollama profile byte-identical.
- Routing: llamacpp joins the loop, `wires_tools` and in-process dispatch.
- Widened rejected-tools fallback: 4xx/5xx mentioning `tools` or `jinja`, only before any round; llamacpp answers as text with a notice once per pane. Ollama keeps its current silent fallback.
- Round caps as Ollama: 8, or 24 with a coding family.
- `"llamacpp"` added to `MCP_VERIFIED` last, after the manual check.
- README (Spanish): replace l.362; new "Tools with llama.cpp" section (`--no-jinja` turns tools off).

### Out of Scope
- lmstudio/vllm tools (stay text-only, no routing arm).
- `reasoning_content` decoding (follow-up).
- `--jinja` injection in managed spawn; proactive `/props` capability check; context-size warnings.

## Capabilities

### New Capabilities
- None

### Modified Capabilities
- `chat-engines`: new "llama.cpp native tools" requirement (OpenAI tool-call decode, round body, loop, fallback + once-per-pane notice); stream parsers gain SSE tool-call deltas. Builds on the "Ollama native tools" delta of unarchived `agent-tools`.

## Approach

Approach A from exploration: a small wire-profile enum drives `tool_turn`/`rounds`/`round` in `src/runtime/chat/ollama.rs`. Caps, approvals, `exchange`, relay and timeouts stay shared. Wire details are pinned by a recorded b11935 tool-call SSE fixture.

## Affected Areas

| Area | Impact | Description |
|------|--------|-------------|
| `src/core/ai/chat/stream.rs` | Modified | `OpenAiToolDecoder`, `finish_reason:"tool_calls"` |
| `src/core/ai/chat/adapters.rs` | Modified | `openai_round_body`, shared helper |
| `src/runtime/chat/ollama.rs` | Modified | Wire profile, widened fallback (l.73, l.138) |
| `src/runtime/chat.rs` | Modified | Routing (l.420-426), `wires_tools`, dispatch (l.793), notice |
| `src/core/ai/chat/engine.rs`, `engine/tools_tests.rs` | Modified | `MCP_VERIFIED` (S4) |
| `tests/fixtures/llamacpp/` | New | b11935 tool-call fixture |
| `README.md` | Modified | l.362, new section |

## Delivery Slicing (stacked-to-main; production lines only)

1. PR 1: S1 decoder + S2 body (~240-320).
2. PR 2: S3 loop, routing, fallback + notice (~100-140).
3. PR 3: S4 gating + README, after the manual check.

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| S4 enables tools for every llamacpp user with tools on | Med | Last slice, manual check first; `--no-jinja` or tools off |
| Unconfirmed wire (fragments, `id`/`index`, no-jinja error) | High | Recorded b11935 fixture before S1 decoder is final |
| S3 regresses Ollama | Med | Existing Ollama tests and bodies unchanged |
| Small models misuse tools / loop | Med | Existing refusals, round cap, approvals |
| Prompt injection via tool output | Med | Same untrusted-output handling as Ollama |

## Rollback Plan

Revert PR 3 alone to disable llamacpp tools (gate removed, loop dormant); revert PR 2/PR 1 in reverse order. Users: tools off or `--no-jinja`.

## Dependencies

- llama-server with Jinja (default since before b11935); `agent-tools` tool registry and Ollama loop.

## Success Criteria

- [ ] Fixture-driven tests: llamacpp turn executes a tool call and continues.
- [ ] All Ollama tests pass with byte-identical request bodies.
- [ ] `--no-jinja` server: text answer plus one notice per pane.
- [ ] Manual: Qwen3.8-27B IQ4_XS and Qwen2.5-7B-Instruct find a directory and type `cd` after approval.
