# Proposal: Managed llama-server options

## Intent

Answers to the open design points of the managed llama-server (0.19.0): a missing model file gets a clear message, extra arguments can be passed, and an idle server stops on its own. Only llama-server is managed; no other server is.

## Scope

In scope:
- Before each start, after `llama-server` is found, the model file must exist; otherwise nothing starts and the turn reads `llama-server not started: model file not found: <path>`. Not checked at config load.
- `args = [...]` under `[ai.servers.llamacpp]`: extra arguments, config file only, one argv element each, no shell; entries that clash with what lazarobox sets (`--host`, `--port`, `-m`) are refused.
- An idle stop: the server stops after an idle time (default 15 min) and starts again on the next llamacpp turn; set in the config and in the menu (5, 15, 30 min, never).
- One server per model, each on its own port (the configured one and the seven after it): a second lazarobox, or a llama-server the user started, serving the same model is used as it is; another model starts its own server on the next free port instead of being refused. Only a listening socket of the same user is used. The picker keeps offering the local models when another llama-server answers.

Out of scope:
- Managing Ollama, LM Studio or vLLM.
