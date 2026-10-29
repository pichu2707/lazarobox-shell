# Exploration: llama-server-install

Engram mirror: `sdd/llama-server-install/explore` (id 3381).

## Problem

lazarobox only detects binaries on `PATH`; it never installs them. On a machine without
`llama-server`, `src/core/ai/picker.rs:252-267` hides the `download from Hugging Face…` row
(`PickKey::HfDownload`), although the HF download itself only needs curl
(`src/runtime/hf.rs:1-4`). The user cannot download models nor run llama.cpp at all.

## Current state

### Detection
- `src/core/ai/mod.rs:83-98` CATALOG: `server("llamacpp", 8080, "/v1/models", Some("llamacpp"), "llama-server")`.
- `src/core/ai/mod.rs:325-345` `server_state`: models reply → Available; no reply + `on_path` → NotRunning; else NotInstalled.
- `src/runtime/ai_detect.rs:75-83` `find_on_path(name, path)`: absolute PATH entries only, executable regular file.
- `src/runtime/ai_detect.rs:585-592` `probe` uses `CliEnv.path` (`src/runtime/chat/cli.rs:80-82`).
- `src/runtime/ai_detect.rs:530-544` `local_models` only when llamacpp != NotInstalled.
- `src/core/ai/mod.rs:103-113` `start_command`: copy-only hint, never run.

### Managed spawn
- `src/runtime/chat/managed.rs:217-230` hardcodes `llama-server`; `:411-417` resolves it with `find_on_path(name, &self.path)`.
- Config `[ai.servers.llamacpp]` (`src/core/ai/config.rs:319-345, 385`).

### Data dirs
- `$XDG_DATA_HOME` read at `src/runtime.rs:303, 344`; `CliEnv.data` = `<data home>/lazarobox` (`cli.rs:85`); helper in `src/core/desktop.rs:124-131`.

## Reusable pieces (HF download)

- `src/runtime/hf.rs`: `Curl` (`-q`, https-only incl. redirects, absolute-PATH curl, process group kill), `download_args` with resume, `download()` loop with 250 ms progress and oneshot cancel, `keep()` size + sha256 check off the event loop (`sha2` already a dependency), temp file + rename, `space_check`.
- `src/core/ai/picker/hub.rs`: picker `Screen` state machine with a y/n Confirm screen and a cancel prompt. Best fit for a step-by-step installer.
- Global y/n modal `InputMode::Confirm` (`src/app.rs:112-140`, `on_confirm_key` l.2535).

## Archive extraction

No zip/tar/flate crate in `Cargo.toml`. The project shells out (curl, ps, gh). Shelling out to
`tar -xzf <archive> -C <fresh empty dir> --no-same-owner` after validating `tar -tzf` members
(reject absolute and `..` paths) fits the project; GNU vs BSD tar differences must be guarded.

## Upstream facts (GitHub API, 2026-10-27)

- `GET /releases/latest` returns tag `v0.7.0` with only `nightly-tag.txt`: NOT usable.
- Builds are tagged `bNNNN` (b11935 latest), all `prerelease: true`. Pick the newest `^b\d+$` tag from `GET /releases?per_page=N` that has the wanted asset.
- Asset pattern: `llama-<tag>-bin-<platform>[-<variant>][-<arch>].tar.gz|zip`.
  - Linux x64: `ubuntu-x64`, `ubuntu-vulkan-x64`, `ubuntu-cuda-{12.8,13.4}-x64` (+ separate `cudart-…` archive), `ubuntu-rocm-10.0-x64`, openvino, sycl.
  - Linux arm64: `ubuntu-arm64`, `ubuntu-vulkan-arm64`, `ubuntu-cuda-13.4-arm64`, `linux-arm64-snapdragon`.
  - macOS: `macos-arm64`, `macos-x64` (Metal built in).
  - Windows: `.zip` variants (out of scope: Windows unsupported).
- Integrity: each asset has `"digest": "sha256:<hex>"` in the release JSON; no SHA256SUMS file; GitHub build attestations exist (`gh attestation verify`, optional).
- Archive layout NOT verified: reportedly `llama-server` next to `libllama.so`/`libggml*.so`; rpath/`$ORIGIN` unknown; top-level folder name unknown. Must be verified in design with a real download (`readelf -d`).
- Ubuntu-built binaries may need a newer glibc/libstdc++ than old distros; musl unsupported.

## Variant detection

- Linux: `nvidia-smi -L` / `/proc/driver/nvidia/version` (CUDA), Vulkan ICD files in `/usr/share/vulkan/icd.d` or `/etc/vulkan/icd.d`, `/dev/kfd` (ROCm, flaky), else CPU.
- macOS: arch from `std::env::consts::ARCH`, Metal automatic.
- Probes must not go through a shell.

## Platforms and constraints

- Linux supported, macOS unofficial (README:61), Windows unsupported (README:62); WSL = Linux.
- CI: ubuntu-latest, macos-14, macos-15-intel.
- The jail (`src/core/ai/jail.rs`) covers CLI turns only, not llama-server.
- Install under `<XDG_DATA_HOME>/lazarobox/llama.cpp/<tag>/` with a `current` link; keep it separate from the model cache `~/.cache/llama.cpp`.

## Plug-in point

Add `<data>/lazarobox/llama.cpp/current/` as an extra PATH entry for `find_on_path` callers.
`CliEnv.path` and `Managed.path` are startup snapshots: after install the app must refresh them
and re-run detection (`spawn_detection`, `force=true`). This is the riskiest wiring point.
`start_command` must show the absolute path for an owned install. If there is no rpath,
`Managed::spawn` sets `LD_LIBRARY_PATH`/`DYLD_LIBRARY_PATH` for that child only.

## Approaches

| # | Approach | Pros | Cons | Effort |
|---|---|---|---|---|
| A | Prebuilt GitHub release into a lazarobox-owned dir (curl + sha256 digest + `tar`) | Reuses the HF pipeline; no new deps; no root; reversible; variant selectable | Own update/uninstall; glibc/rpath risks; `/latest` quirk; trust = GitHub TLS + digest | Medium |
| B | Package manager (brew, winget, nix; apt has none) | Trusted channel, PM updates | sudo/brew needed; nothing usable on Debian/Parrot; privileged installs from a TUI | S (show) / L (run) |
| C | Build from source | Host-optimised | Toolchain, minutes of build, big failure surface | Large |
| D | Decouple the HF row from llama-server being installed | Unblocks model downloads today | Installs nothing | XS |

## Recommendation

D as its own first slice, A as the feature, B as an "I'll do it myself" step that only shows a
copy command. Guided flow, y/n at each step:

1. Show plan, OS/arch and variant (CPU, or Vulkan if an ICD is found).
2. Resolve release (newest `bNNNN` with the asset, its digest); show tag, size, target dir, free space.
3. Download with progress and cancel prompt.
4. Verify sha256; on mismatch delete and stop.
5. Extract to a fresh temp dir, validate members, run `llama-server --version`, rename into `llama.cpp/<tag>/`, update `current`.
6. Re-detect: engine becomes NotRunning; HF row and managed config usable.

Generalise `runtime/hf.rs::Curl` (per-request base URL; GitHub hosts) instead of copying it.

## Open product questions

1. Platforms in v1: Linux only, or Linux + macOS?
2. Variants in v1: CPU, CPU + Vulkan, or also CUDA/ROCm?
3. System vs owned llama-server: which wins?
4. Pinned tag vs newest; update step; notify on newer tag?
5. Verification bar: sha256 digest only, or also `gh attestation verify` when available?
6. Offer the package-manager route as a show-only step?
7. Ship the decoupling fix first as its own PR?
8. Disk: warn or block on low space; uninstall in v1?
9. Where the flow lives in the UI.
10. Offer to enable `managed = true` after install?

## Risks

- Binary trust: upstream binary runs unsandboxed as the user; digest only proves integrity over the API channel.
- `/releases/latest` trap and all builds flagged prerelease.
- Archive layout and shared libs unverified.
- glibc/libstdc++ mismatch on older distros.
- Tar path traversal and symlink members.
- GitHub API rate limit (60 req/h unauthenticated).
- PATH snapshot refresh at runtime.
- Disk cleanup of failed installs and old tags.
- Tests need an injectable base URL / local stub; `tar` in CI.
- README and `mod.rs:100-102` docs state lazarobox never installs or starts things; needs a documented exception.
- PR budget (production lines only): suggested slices (1) decouple HF row, (2) core logic, (3) runtime, (4) UI + docs.
