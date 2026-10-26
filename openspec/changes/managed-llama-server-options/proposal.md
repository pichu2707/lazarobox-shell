# Proposal: Managed llama-server options

## Intent

Answers to the open design points of the managed llama-server (0.19.0): a missing model file gets a clear message, extra arguments can be passed, and an idle server stops on its own. Only llama-server is managed; no other server is.

## Scope

In scope:
- Before each start, after `llama-server` is found, the model file must exist; otherwise nothing starts and the turn reads `llama-server not started: model file not found: <path>`. Not checked at config load.
- `args = [...]` under `[ai.servers.llamacpp]`: extra arguments, config file only, one argv element each, no shell; entries that clash with what lazarobox sets (`--host`, `--port`, `-m`) are refused.
- An idle stop: the server stops after an idle time (default 15 min) and starts again on the next llamacpp turn; set in the config and in the menu (5, 15, 30 min, never).

Out of scope:
- Managing Ollama, LM Studio or vLLM.
- Sharing one managed server between two lazarobox instances.
