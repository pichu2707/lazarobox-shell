# Apply progress: llamacpp-tools

## Batch 1 (PR1, branch feat/llamacpp-tools-pr1)

Done: T1.7 (byte pin of the Ollama request body, test-only, no production change).

- `src/core/ai/chat/adapters/tests.rs`: four `pin_*` tests. Golden bytes for no rounds + system + memory, one round without ids/index/thinking, one round with ids/index/thinking (two calls), and tools-on = tools-off bytes plus a trailing `,"tools":[...]` (structural, so registry text changes do not break it).
- Mutation check: perturbing two golden strings made two pins fail; restored.

Remaining PR1: T1.0 (manual, blocked on user GPU), T1.1-T1.6, T1.8-T1.10.
