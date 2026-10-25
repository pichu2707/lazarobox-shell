# Proposal: Pick a local model for llama-server

## Intent

Choosing `llamacpp` in the picker while llama-server is not running only said how to start it by hand. The user wants to pick a model and have lazarobox start what is needed. The managed llama-server (managed-llama-server) already starts and stops the server, but it had to be turned on by editing `config.toml`.

## Scope

In scope:
- Detection lists the `.gguf` files in the usual folders when llama-server is installed.
- Level 2 of a stopped llamacpp lists the managed model and those files, marked `lazarobox starts it`.
- Choosing one writes `managed = true` and `model` under `[ai.servers.llamacpp]` on save, keeping the other keys.
- The save starts llama-server in the background at once, with no restart; the server it replaces stops for good.

Out of scope:
- Downloading models (issue #282).
- Other servers.

## Approach

`core::ai::gguf` holds the pure folder and file rules; `runtime::gguf::find` walks the folders (no symlinked folders, bounded depth and entries) inside detection, into `Detection.local_models`. `AiMenu::drive` calls `AiConfig::manage` on a local pick, and `MenuState::edits` writes the two keys. The runtime keeps the ports and the managed server in `chat::LiveServers`, read by each turn and detection; `App` emits `Effect::StartManaged` when a save turns it on or changes its model, and shows `AppEvent::ManagedStarted` in the statusline.

## Risks

- The menu now writes under `[ai.servers]`, which it used to leave alone: only `managed` and `model` of llamacpp, through the same key-preserving editor.
- lazarobox starts a program on a menu save. It is the user's explicit choice of a file found on the machine, run as before: one argv element per argument, `127.0.0.1` only, its own process group.
