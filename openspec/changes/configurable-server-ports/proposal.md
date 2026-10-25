# Proposal: Configurable server ports (phase 1)

## Intent

A local model server is only usable when it listens on its catalog port. Recent `llama-server` builds default to another port (b11815: 9931), and the catalog port can be taken by an unrelated service (an Apache app on `:8080` on the owner's machine). Let the user set the port of each server engine in `config.toml`, used by both detection and chat, and tell the user, when they select an installed server that is not running, where lazarobox looks and how to start it.

## Scope

In scope:
- `[ai.servers.<id>] port = N` for `ollama`, `lmstudio`, `llamacpp` and `vllm`; default the catalog port. Read at startup, never written by the menu.
- Detection and chat use the same port; the host stays `127.0.0.1`.
- The picker lists installed-but-not-running servers and, on their model level, shows where lazarobox looks and a copyable start command.

Out of scope (phase 2):
- lazarobox starting, stopping or supervising a server, or choosing its model (see the Hugging Face item, issue #282).
- Custom hosts or endpoints.

## Approach

`ServerPorts` (core) holds the overrides; `AiConfig` reads it; the runtime passes one copy to the detector (a catalog with the ports replaced) and one to the chat runner (`runner_with_ports`). `runner_with(port: Option<u16>, ..)` keeps its signature for the integration tests.

## Risks

- Detection may now probe a user-chosen loopback port. It stays GET-only, `127.0.0.1` only, and the owner check still rejects a foreign service.
