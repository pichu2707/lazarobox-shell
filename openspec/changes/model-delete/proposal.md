# Proposal: Model Delete

## Intent

Local models take gigabytes, and lazarobox can download them from the menu (`ollama-download`, `model-download`) but cannot remove them. The user has to leave for `ollama rm` or find a file inside the Hugging Face cache by hand. This change lets the user delete a downloaded model from the row where the menu lists it, after a confirmation that names what goes and how much space it frees. Refs #434.

**For:** anyone running local models with Ollama or llama.cpp from the menu.

## Scope

### In Scope
- **Ollama models:** deleted through the local server, `DELETE /api/delete` with `{"model": name}` on `127.0.0.1` at the port lazarobox already uses for that server. lazarobox never touches Ollama's store itself.
- **GGUF files of llama.cpp:** the file the row names, inside the folders the model scan reads.
- **A split model goes whole:** all its parts, since the list shows only part 1. Deleting part 1 alone would leave the rest as orphans the list no longer shows.
- **The Hugging Face layout is kept consistent:** the snapshot link and its blob are removed together. A blob another snapshot still links to stays. A repository folder with no snapshot files left is removed.
- **A confirmation screen** in the picker, on the template of `Remove llama.cpp` (`ConfirmRemove`): it names the model, the folder and the size, is armed only once drawn, and only an unmodified `y` deletes. Any other key goes back.
- **Refused while it would break something:** see D3 for models in use. Always refused while a download runs, as other picker actions are (`Outcome::Busy`), because a running download cannot be matched to a repository.
- **After a delete:** the engines are detected again, the cursor stays near its row, and a notice says what was deleted and how much was freed. A failure says why and deletes nothing more.
- **Docs:** README, CHANGELOG, footer hint of the Models level.

### Out of Scope
- Moving to a trash folder or undoing a delete. A delete is final, and the screen says so.
- Deleting several models at once.
- The `.incomplete` files of a cancelled download, kept so it can resume, and `mmproj` companion files, which the list does not show.
- Models of a running llama-server that lazarobox does not manage: the list only has their names, not a file.
- Remote or non-default Ollama hosts. lazarobox never reads `OLLAMA_HOST`; it talks only to loopback.
- Deleting the engines themselves. `Remove llama.cpp` already exists.

**Known limitation:** Ollama shares layers between models, so the size it lists can be more than what a delete really frees.

## The Delete Flow

1. On the Models level of a local engine, the selection is on a downloaded model. The footer shows `d delete`.
2. `d` opens the confirmation: name, folder, size, and `This cannot be undone. y delete · any other key back`.
3. `y` starts the delete on the runtime and shows `Deleting…`.
4. The runtime checks the target again (Safety below) and deletes it.
5. The reply shows the result, the engines are detected again, and the list no longer has the row.

## Safety

The delete is a runtime effect with its own checks; the core only asks for it:

- The target is resolved with symlinks followed, and must lie inside one of the scanned folders, also resolved. A path from the config or from an assignment gets no shortcut: `PickKey::Model` can hold the managed model's path, and the existing `is_owned` check is only lexical.
- The file must be a regular `.gguf` that the scan would list. A Hugging Face link must resolve to a blob inside that same repository folder.
- Only files and folders left empty are removed. There is no recursive delete.
- The Ollama call has its own gate beside `pull.rs`: one fixed path, on `127.0.0.1` only, sending the name in a JSON body. The name is never part of a URL or a path.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `ollama-download`: delete through the local server.
- `model-download`: delete of files in the Hugging Face cache and the other scanned folders.
- `config-menu`: Task picker, Footer (the `d` hint), the confirmation screen.
- `ai-task-models`: what happens to an assignment (D3).

## Approach

Reuse what exists:
- **Confirmation:** a new screen beside `Level::Install(Wizard)`, with the same arming and reply handling (`in_flight`, stale replies dropped).
- **Detection:** `request_forced_detection`, as a finished download does.
- **Cursor:** `reanchor`.

New parts:
- `src/runtime/loopback/delete.rs`, a sibling of `pull.rs`. The generic `request_to` refuses a `DELETE` with a body.
- A runtime module that resolves, checks and removes GGUF files.
- Sizes: `/api/tags` already answers `size`, which today is not kept (`EngineState.models` is `Vec<String>`), and a GGUF size is one `stat` per part (D4).
- A hint for the row: `HINT_PICKER` is shared by Engines and Models, so the footer needs a branch before it.

Expected delivery is two PRs:

| PR | Scope | Estimate (production lines) |
|----|-------|-----------------------------|
| 1 | Confirmation screen, `d` key, hint, in-use refusals, Ollama delete | ~300 |
| 2 | GGUF delete: checks, split parts, Hugging Face layout | ~250 |

## Affected Areas

| Area | Impact |
|------|--------|
| `src/core/ai/picker.rs`, `src/core/ai/picker/` | `d` on a row, confirmation screen, in-use check |
| `src/core/menu.rs` | Key, footer hint, effect |
| `src/core/ai/mod.rs`, `src/core/ai/parse.rs` | Ollama sizes |
| `src/runtime/loopback/delete.rs` | New: the Ollama call |
| `src/runtime/gguf.rs` or a new module | New: checks and removal |
| `src/app/`, `src/runtime.rs` | Effect, reply, detection |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| A delete removes a file outside the model folders | Low | Resolved path must lie inside a resolved scanned folder; regular `.gguf` only; no recursion; tests with symlinks that point outside |
| The shared Hugging Face cache loses a model another tool uses | Med | The confirmation names the folder; D1 |
| A shared blob is removed and breaks another snapshot | Low | A blob with another link stays |
| A model is deleted while a turn uses it | Med | Refused (D3); the assignment check reads the draft and the saved config |
| A key meant for something else deletes | Low | Two steps; only an unmodified `y` on an armed screen; held keys ignored |
| Ollama answers 404 because the name was sanitized or cut | Low | Reported as a failure; detection runs again |

## Rollback Plan

A plain revert. No config key and no stored format change. Models already deleted do not come back: they can be downloaded again.

## Success Criteria

- [ ] `d` on a downloaded Ollama model, then `y`, removes it, and `ollama list` no longer shows it.
- [ ] `d` on a GGUF row, then `y`, removes all its parts and frees the space.
- [ ] Any key other than `y` deletes nothing.
- [ ] A symlink that points outside the scanned folders is refused.
- [ ] A blob another snapshot links to survives.
- [ ] A row with no file behind it offers no `d`.

## Evidence

From the worktree at `81e0a5a2`:

- **Rows:** `listed_models`, `model_rows` (`src/core/ai/picker.rs:309,355`). The managed path can be a listed row (`picker.rs:176`).
- **Scanned folders:** `model_dirs` (`src/core/ai/gguf.rs:26`). `is_model_file` skips `mmproj` and every part but the first (`gguf.rs:60`).
- **No size kept:** `EngineState.models` is `Vec<String>` (`src/core/ai/mod.rs:269`). `/api/tags` answers `size` per model ([Ollama API](https://github.com/ollama/ollama/blob/main/docs/api.md)).
- **No manifest:** Hugging Face downloads use the plain hub layout (`src/runtime/hf.rs:545,677`), and nothing records who downloaded a file.
- **Ollama delete:** `DELETE /api/delete` with `model` in the body; 200 on success, 404 when missing ([Ollama API](https://github.com/ollama/ollama/blob/main/docs/api.md)). Live on Ollama 0.40.3, it answered 200 for `smollm2:135m`.
- **Loopback gate:** `request_to` refuses a `DELETE` with a body (`src/runtime/loopback.rs:628`). The pull has its own gate (`src/runtime/loopback/pull.rs:20,57`).
- **Template:** `Remove llama.cpp` (`src/core/ai/picker/manage.rs:63`, `src/core/ai/picker/install.rs:116,338`). `is_owned` is lexical.
- **Keys:** `d` is free in the picker; it is bound only on the account row of the task list (`src/core/menu.rs:2210`). Hint: `HINT_PICKER` (`menu.rs:1040,2890`).
- **In use:** assignments (`src/core/ai/config.rs:408`), managed model (`config.rs:462`), a running download (`src/core/ai/picker/hub.rs:58`).
- **Detection:** `request_forced_detection` (`src/app.rs:1272`), called after a download (`src/app/hub.rs:120`).

## Decisions

The owner answered each question on 2026-11-09, taking the recommended option every time.

- **D1. Every GGUF file the list shows can be deleted.** lazarobox cannot tell its own downloads from those of the `hf` CLI, which share the same layout, and the confirmation names the folder. Another tool that uses the shared cache has to download that model again.
- **D2. Entry point: `d` on the model's row,** with the footer showing it.
- **D3. A model in use is refused.** When a task uses it, when it is the managed llama-server's model, or when a running chat turn uses it, nothing is deleted and a notice says who uses it. The user reassigns first.
- **D4. The size is shown.** The confirmation shows the size and the notice what was freed. Ollama sizes are kept from `/api/tags`, and each GGUF part needs a `stat`. For Ollama the size is approximate, because models share layers.
