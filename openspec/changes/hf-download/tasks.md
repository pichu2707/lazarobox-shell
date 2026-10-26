# Tasks: Hugging Face downloads (issue #282)

Feature document: `odd/tasks/hf-download.md` (local, not in the package).

- [x] 1.1 Proposal and spec deltas (model-download, ai-task-models, config-menu).
- [x] 2.1 RED/GREEN: Hub JSON read (search, repo info, tree) into plain types; malformed or oversized replies fail with `Hugging Face request failed: <reason>`.
- [x] 2.2 RED/GREEN: GGUF choices (mmproj dropped, splits grouped and complete only, quant label, decimal sizes, sorted by size).
- [x] 2.3 RED/GREEN: license id and link (license_link, LICENSE* at the commit, model page; `not stated`).
- [x] 2.4 RED/GREEN: token lookup order, trimmed, blank is none; hub folder lookup; repo folder and blob/snapshot/ref paths.
- [x] 3.1 RED/GREEN: curl arguments for API GETs and downloads (`-q`, HTTPS only, `--` URL, no token in argv; token once on stdin); status to message mapping.
- [x] 3.2 RED/GREEN: download into the cache with a fake curl: `.incomplete`, size check, rename, relative snapshot link, `refs/main`; existing blob skipped; resume with `--continue-at -`; cancel keeps the partial file; process group registered with the watcher.
- [x] 3.3 RED/GREEN: free space check before the start; progress about every 250 ms.
- [x] 4.1 RED/GREEN: one download at a time (runtime wiring); `download from Hugging Face…` row last on the llamacpp model level; progress row while running; cancel prompt.
- [x] 4.2 RED/GREEN: Query, Repos, Files and Confirm levels; repo id vs search; info rows; footers; errors stay on their level; late replies dropped; nothing downloads before `y`.
- [x] 4.3 RED/GREEN: status bar progress, done, failure and cancel; redetect once on success.
- [ ] 5.1 Docs: README (flow, token, what is sent, where it is stored, how to delete), CHANGELOG, footers.
- [ ] 5.2 [manual] Live: real download of `Qwen/Qwen2.5-0.5B-Instruct-GGUF` `Q2_K`, cancel and resume, listed by the picker, chat answers through the managed llama-server.
- [ ] 5.3 [manual] Live: a gated repo without a token says `set HF_TOKEN`; with `HF_TOKEN` it downloads.
