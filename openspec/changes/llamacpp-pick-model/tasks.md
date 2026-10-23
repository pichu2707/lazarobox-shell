# Tasks: Pick a local model for llama-server

- [x] 1.1 RED/GREEN: with `managed = true`, the picker offers the configured model while llama-server is not running (PR #301).
- [x] 1.2 RED/GREEN: the model folders and file rules; the walk (nested, HF snapshot links, symlinked folders not followed, broken links, depth limit); detection brings them only with llama-server installed.
- [x] 1.3 RED/GREEN: level 2 of a stopped llamacpp lists the managed model and the files; a pick turns the managed server on in the draft and the save writes `managed` and `model`, keeping `port`.
- [x] 1.4 RED/GREEN: a save that turns it on starts it at once; turns and detection read the live ports and server; a replaced server stops and never starts again.
- [x] 1.5 Docs (README, CHANGELOG) and gate.
- [ ] 1.6 [manual] With a real `llama-server`: pick the model from the menu, save, see `llama-server is ready`, chat with it, quit and find no server left.
