# Proposal: Guided llama-server install

## Intent

Without `llama-server` on PATH, the llamacpp engine is `not installed`, the `download from Hugging Face…` row is hidden (`src/core/ai/picker.rs:252-267`) although the download only needs curl, and the user cannot run llama.cpp at all. Let lazarobox install the upstream prebuilt `llama-server` through a guided flow where every step asks y/n first. This is a documented exception to "lazarobox never installs anything".

## Scope

### In Scope
- Slice 1: the HF row shows for llamacpp even when `not installed`, with a note that `llama-server` is needed to run downloaded models.
- `install llama-server…` entry on the llamacpp row when `not installed`; opens the wizard.
- Wizard, y/n per step: plan (OS/arch, variant CPU, or Vulkan offered when an ICD is found) → resolve release (newest `^b\d+$` tag with the asset; never `/releases/latest`) showing tag, size, target dir, free space → download with progress and cancel → sha256 from the asset `digest` (mismatch: delete, stop) → `gh attestation verify` if `gh` is on PATH (failure aborts; absent: warn, continue) → extract, validate, `llama-server --version` → re-detect → offer `managed = true` → offer HF download.
- Copy-only alternative ("I'll do it myself"), e.g. `brew install llama.cpp`; never run.
- Disk check blocks when free space < archive + extraction estimate.
- Install to `<XDG_DATA_HOME>/lazarobox/llama.cpp/<tag>/` + `current` link; owned dir appended LAST to PATH (system binary wins).
- Pinned tag; notify when a newer `bNNNN` exists; user re-runs the flow.
- `remove llama-server` in the same menu; prunes old tags.
- Platforms: Linux x64/arm64, macOS arm64/x64.

### Out of Scope
- CUDA, ROCm, SYCL, OpenVINO, Snapdragon variants; Windows.
- Automatic updates; running package managers; building from source.

## Capabilities

### New Capabilities
- `llama-server-install`: release resolution, variant choice, download, verification, extraction, owned dir layout, update notice, removal, wizard flow.

### Modified Capabilities
- `config-menu`: HF row no longer requires llama-server (delta of unarchived `hf-download`); install/remove entries on the llamacpp row.
- `ai-engine-detection`: owned dir as last PATH entry; runtime PATH refresh and forced re-detection after install/remove.
- `chat-engines`: managed spawn resolves the owned binary; shared-lib env for that child only if no rpath.

## Approach

Approach A from exploration. Generalise `src/runtime/hf.rs` (`Curl` with per-request base URL, `download`, `keep`, `space_check`) instead of copying. Pure logic (tag/asset selection, digest parsing, member validation, plan) in `src/core/ai/`. Shell out to `tar -tzf` (reject absolute, `..`, unsafe links) then `tar -xzf -C <fresh dir> --no-same-owner`; rename into place. Wizard reuses the picker `Screen` state machine (`picker/hub.rs`).

## Affected Areas

| Area | Impact | Description |
|------|--------|-------------|
| `src/core/ai/picker.rs`, `picker/hub.rs` | Modified | HF row gate; install/remove screens |
| `src/core/ai/mod.rs` | Modified | `start_command` absolute path; doc comment l.100-102 exception |
| `src/core/ai/install.rs` (new) | New | Pure install logic |
| `src/runtime/hf.rs` | Modified | Shared curl/download pieces |
| `src/runtime/llama_install.rs` (new) | New | GitHub API, extract, verify, gh |
| `src/runtime/ai_detect.rs`, `chat/cli.rs`, `chat/managed.rs` | Modified | PATH extension, refresh, lib env |
| `README.md` (Spanish) | Modified | Exception documented |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Unsandboxed upstream binary | Med | Explicit consent, digest, optional attestation |
| Archive layout / rpath unknown | High | Verify with real download in design (`readelf -d`) |
| glibc too old | Med | `--version` check before activation; clear error |
| Tar traversal/symlinks | Med | Member validation, fresh dir, GNU/BSD guard |
| Stale PATH snapshots | High | Explicit refresh + `force` re-detection, tested |
| GitHub rate limit (60/h) | Low | Few calls; readable error |

## Rollback Plan

Each slice reverts independently via `git revert`. Users: `remove llama-server` or delete `<XDG_DATA_HOME>/lazarobox/llama.cpp/`; a system binary is never touched.

## Dependencies

- GitHub releases of `ggml-org/llama.cpp`; `tar`, `curl`; optional `gh`.

## Delivery Slicing (chained PRs; budget counts production lines only)

1. HF row decoupling. 2. Core pure logic. 3. Runtime (download, verify, extract, PATH, managed env). 4. UI wizard + README/doc comment.

## Success Criteria

- [ ] On a clean host, llamacpp goes `not installed` → `not running` via the wizard, with no step taken unasked.
- [ ] Digest or attestation failure leaves no files behind.
- [ ] A system `llama-server` keeps priority.
- [ ] Remove restores `not installed` (absent a system binary).
