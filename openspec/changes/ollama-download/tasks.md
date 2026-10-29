# Tasks: Ollama downloads

Feature document: `odd/tasks/ollama-download.md` (local, not in the package).

- [x] 1.1 Proposal and spec deltas (ollama-download, ai-task-models, ai-engine-detection, config-menu).
- [x] 2.1 RED/GREEN: library page parser (fixture of the real page): cards, sizes, capabilities, pulls; unsafe names skipped; embedding and cloud-only left out.
- [x] 2.2 RED/GREEN: fit rule on size tags (reuse `hf::fits`, 5 bits per parameter); Popular list of at most 20; curated fallback with its info row.
- [x] 2.3 RED/GREEN: manifest sizes, license title from the license layer; name and tag validation.
- [x] 3.1 RED/GREEN: curl for ollama.com and registry.ollama.ai (HTTPS only, caps, `--` URL, no shell) with a fake curl.
- [x] 3.2 RED/GREEN: loopback client allows POST `/api/pull` and still refuses other POST paths and hosts.
- [x] 3.3 RED/GREEN: pull NDJSON decoder (status, digests, total/completed, success, error); pull worker with a fake server: progress, success, error, cancel closes the connection.
- [ ] 4.1 RED/GREEN: `download from Ollama…` row, Popular, Sizes and Confirm screens, late replies dropped, nothing pulled before `y`.
- [ ] 4.2 RED/GREEN: one download at a time across Hugging Face and Ollama; progress row, status bar, cancel prompt; forced detection once on success.
- [ ] 5.1 Docs: README (flow, what is sent, where Ollama stores models, the page may change), CHANGELOG, catalog module doc.
- [ ] 5.2 [manual] Live: pull `qwen3:4b` from the Popular screen, cancel and pull again (resume), the model is listed and chats.
- [ ] 5.3 [manual] Live: check that Ollama really stops a pull when the client closes the request (the cancel assumption).
