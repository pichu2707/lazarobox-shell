# Tasks: Configurable server ports

- [x] 1.1 RED/GREEN: `[ai.servers.<id>] port` parsed with defaults, problems and unknown ids ignored; menu saves keep the table byte for byte.
- [x] 1.2 RED/GREEN: detection probes each server on its configured port; a chat turn goes to the configured port of its engine.
- [x] 1.3 RED/GREEN: the picker lists installed-but-not-running servers and shows a copyable start note on their model level.
- [ ] 1.4 Docs (README, CHANGELOG) and gate.
- [ ] 1.5 [manual] With `[ai.servers.llamacpp] port = 39080` and a real `llama-server --port 39080`, the menu shows `llamacpp available · 1 model` and a chat turn streams.
