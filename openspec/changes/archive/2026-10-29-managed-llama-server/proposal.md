# Proposal: Managed llama-server (phase 2 of configurable server ports)

## Intent

A local llama.cpp server only works while the terminal tab that runs it stays open. Closing that tab stops the engine, and nothing in lazarobox brings it back. Let lazarobox start its own `llama-server` when a llamacpp chat turn needs it, and stop it when lazarobox quits, on a fixed uncommon port.

## Scope

In scope:
- Opt-in `managed = true` and `model = "<path to a .gguf>"` under `[ai.servers.llamacpp]`. While managed, llamacpp's port defaults to 39281; `port` still overrides it.
- The first llamacpp turn starts `llama-server --host 127.0.0.1 --port <port> -m <model>` and waits until `GET /v1/models` answers 200. Later turns reuse it, and a server that ended is started again.
- Quitting lazarobox stops it, whole process group. On Linux, the kernel also kills it when lazarobox dies without quitting.
- A port that already answers before a start belongs to someone else: nothing starts and the turn says so.
- The chat gate lets a managed llamacpp through when its binary is installed, running or not, without looking up the model name, which llama-server ignores.
- Fix found on the way: closing the terminal made lazarobox abort before its quit cleanup, which left every chat server it started running.

Out of scope:
- Other servers (ollama, lmstudio, vllm).
- An idle timeout; only quitting stops the server.
- Picking or downloading the model from the menu (see the Hugging Face item, issue #282).
- A leftover sweep for macOS, which has no parent-death signal: a crash there can leave the server running.

## Approach

`AiConfig` reads `ManagedServer { model }` and moves llamacpp's port to `MANAGED_PORT`. `runtime::chat::managed::Managed` owns the process: `ensure()` starts it at most once at a time and `shutdown()` kills its group. `runner_managed` calls `ensure()` before a llamacpp turn, `Chats::with_managed` stops it in `Chats::shutdown`, and `availability_with` treats an installed managed llamacpp as ready.

## Risks

- lazarobox now executes a program for chat. It is opt-in, runs only from a chat turn (never on config load or detection), uses one argv element per argument with no shell, binds `127.0.0.1` only and runs in its own process group.
- The parent-death signal follows the thread that started the server. The runtime is multi-threaded with no `block_in_place`, so that is a worker that lives as long as lazarobox.
