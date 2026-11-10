# Tasks: Configurable server ports

- [x] 1.1 RED/GREEN: `[ai.servers.<id>] port` parsed with defaults, problems and unknown ids ignored; menu saves keep the table byte for byte.
- [x] 1.2 RED/GREEN: detection probes each server on its configured port; a chat turn goes to the configured port of its engine.
- [x] 1.3 RED/GREEN: the picker lists installed-but-not-running servers and shows a copyable start note on their model level.
- [x] 1.4 Docs (README, CHANGELOG) and gate.
- [x] 1.5 [manual] With `[ai.servers.llamacpp] port = 39080` and a real `llama-server --port 39080`, the menu shows `llamacpp available · 1 model` and a chat turn streams.

## Notes

- 1.5: checked in the TUI built from `main` (cd99333). With the server stopped, the picker showed `llamacpp not running` and the note `llamacpp not running on 127.0.0.1:39080 · start it, then press r: llama-server --port 39080 -m <model.gguf>`. After starting it, `r` showed `llamacpp available · 1 model`, saving the chat task kept `[ai.servers.llamacpp]` byte for byte, and a turn streamed.
