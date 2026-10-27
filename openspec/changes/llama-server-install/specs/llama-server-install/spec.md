# llama-server-install Specification

## Purpose

Let lazarobox install the upstream prebuilt `llama-server` (ggml-org/llama.cpp releases) through a guided flow where every step asks y/n first. This is a documented exception to "lazarobox never installs anything". Slice tags: [S2] core pure logic, [S3] runtime, [S4] UI wizard. Scenario tags: [auto] automated with fakes, [manual] needs a real host. Platforms: Linux x64/arm64, macOS arm64/x64. Variants in v1: CPU and Vulkan. Out of scope: CUDA, ROCm, SYCL, OpenVINO, Snapdragon, Windows, automatic updates, running package managers, building from source.

## Requirements

### Requirement: Guided consent per step [S4]
The wizard MUST ask y/n before each step (plan, resolve, download, verify, extract, activate, managed offer, HF offer) and MUST NOT take any step unasked. Answering `n` or Esc at any step MUST stop the flow without side effects beyond cleanup. The wizard MUST also offer a copy-only alternative ("I'll do it myself", e.g. `brew install llama.cpp`) that copies the command text and MUST NEVER run it.

#### Scenario: No step without consent [auto]
- GIVEN the wizard at the plan step
- WHEN the user answers `n`
- THEN no network request is made and no file is created

#### Scenario: Copy-only alternative never executes [auto]
- GIVEN the wizard on a macOS host
- WHEN the user picks "I'll do it myself"
- THEN the suggested command is copied or shown and no process is spawned

### Requirement: Release resolution [S2][S3]
The system MUST resolve the release from the GitHub releases list of `ggml-org/llama.cpp` and MUST NOT use `/releases/latest` (it points to a non-`b` tag). It MUST select the newest release whose tag matches `^b\d+$` and which holds the asset for the chosen platform and variant, prereleases included. Tag selection SHOULD compare the numeric part, not the string. The wizard MUST show tag, asset size, target directory and free space before asking to download.

#### Scenario: Newest matching tag wins [auto]
- GIVEN a releases list with `v0.7.0`, `b9000` (asset present), `b9010` (asset missing) and `b8990` (asset present)
- WHEN the release is resolved
- THEN `b9000` is selected

#### Scenario: Numeric comparison [auto]
- GIVEN tags `b999` and `b1000` both with the asset
- WHEN the release is resolved
- THEN `b1000` is selected

#### Scenario: No usable release [auto]
- GIVEN a list with no `^b\d+$` tag holding the asset
- WHEN the release is resolved
- THEN the flow stops with a readable error and nothing is downloaded

### Requirement: Platform and variant detection [S2][S3]
The plan step MUST detect OS and arch and map them to the upstream asset name. The CPU variant MUST always be offered. Vulkan MUST be offered only on Linux when a Vulkan ICD is found; the user MUST choose the variant. On an unsupported OS/arch the wizard MUST stop with a copy-only alternative.

#### Scenario: Vulkan offered with ICD [auto]
- GIVEN Linux x64 with a Vulkan ICD file present
- WHEN the plan is built
- THEN CPU and Vulkan are offered

#### Scenario: CPU only without ICD [auto]
- GIVEN Linux x64 with no ICD
- WHEN the plan is built
- THEN only CPU is offered

#### Scenario: Unsupported platform [auto]
- GIVEN an OS/arch with no upstream asset (e.g. Windows)
- WHEN the plan is built
- THEN no install is offered and the copy-only alternative is shown

### Requirement: Download and digest verification [S3]
The download MUST show progress and be cancellable. The archive's SHA-256 MUST be computed in Rust (not by parsing `sha256sum` output) and compared with the asset's `digest` (`sha256:<hex>`, case-insensitive hex). A missing or malformed digest MUST abort. On mismatch the archive MUST be deleted and the flow stopped with nothing extracted.

#### Scenario: Digest matches [auto]
- GIVEN a downloaded archive whose SHA-256 equals the asset digest
- WHEN verification runs
- THEN the flow continues to extraction

#### Scenario: Digest mismatch [auto]
- GIVEN an archive whose SHA-256 differs from the digest
- WHEN verification runs
- THEN the archive is deleted, the flow stops and no install directory exists

#### Scenario: Missing digest [auto]
- GIVEN an asset without a `digest` field
- WHEN verification runs
- THEN the flow aborts without extracting

### Requirement: Optional attestation [S3]
When `gh` is on PATH the system MUST run `gh attestation verify` after the digest check; failure MUST abort and clean up. When `gh` is absent it MUST warn and continue.

#### Scenario: Attestation failure aborts [auto]
- GIVEN `gh` present and verification failing
- WHEN the step runs
- THEN the archive is deleted and nothing is installed

#### Scenario: gh absent warns [auto]
- GIVEN `gh` not on PATH
- WHEN the step runs
- THEN a warning is shown and the flow continues

### Requirement: Disk space check [S3]
The system MUST block the download when free space is below archive size plus an extraction estimate, showing the shortfall.

#### Scenario: Insufficient space [auto]
- GIVEN free space smaller than archive plus estimate
- WHEN the resolve step completes
- THEN the download is not offered and the shortfall is shown

### Requirement: Safe extraction [S2][S3]
Members MUST be listed (`tar -tzf`) and validated before extraction. The system MUST reject the archive when any member is an absolute path, contains a `..` component, or is a symlink or hardlink whose target is absolute or escapes the extraction root. Extraction MUST go into a fresh empty directory with `--no-same-owner`, MUST NOT run from inside it, and the result MUST be renamed into place only after validation. The archive is expected to hold one top directory `llama-<tag>/`; relative bare-name symlinks inside it are allowed.

#### Scenario: Traversal member rejected [auto]
- GIVEN an archive listing `llama-b9000/../../evil`
- WHEN validation runs
- THEN the archive is rejected and nothing is extracted

#### Scenario: Absolute member rejected [auto]
- GIVEN a member `/etc/cron.d/x`
- WHEN validation runs
- THEN the archive is rejected

#### Scenario: Escaping symlink rejected [auto]
- GIVEN a symlink member targeting `../../outside` or an absolute path
- WHEN validation runs
- THEN the archive is rejected

#### Scenario: Relative bare-name symlink accepted [auto]
- GIVEN symlink `libllama.so -> libllama.so.0`
- WHEN validation runs
- THEN it is accepted

#### Scenario: Ownership not preserved [auto]
- GIVEN a valid archive
- WHEN extraction runs
- THEN `tar` is invoked with `--no-same-owner` into a fresh empty directory

### Requirement: Post-extract validation and version check [S3]
After extraction the system MUST verify `llama-<tag>/llama-server` exists and is executable, then run `llama-server --version` (no `LD_LIBRARY_PATH` required, the shared library resolves via `$ORIGIN`). On failure (e.g. glibc too old, missing library) the install MUST NOT be activated, the new tag directory MUST be removed and the error shown clearly.

#### Scenario: Version check passes [auto]
- GIVEN an extracted tree whose binary exits 0 on `--version`
- WHEN validation runs
- THEN the install proceeds to activation

#### Scenario: Version check fails [auto]
- GIVEN a binary exiting non-zero with a loader error
- WHEN validation runs
- THEN `current` is untouched, the tag directory is removed and a clear error is shown

### Requirement: Install layout [S3]
Installs MUST live under `<XDG_DATA_HOME>/lazarobox/llama.cpp/<tag>/` (falling back to `~/.local/share` when unset) with a `current` link pointing to the active tag. `current` MUST switch only after a successful version check, atomically. Re-installing an existing tag MUST NOT corrupt the active install.

#### Scenario: Layout after install [auto]
- GIVEN a successful install of `b9000`
- WHEN it finishes
- THEN `<data>/lazarobox/llama.cpp/b9000/llama-b9000/llama-server` exists and `current` resolves to `b9000`

#### Scenario: XDG fallback [auto]
- GIVEN `XDG_DATA_HOME` unset
- WHEN the target is computed
- THEN it is under `~/.local/share/lazarobox/llama.cpp/`

### Requirement: PATH plug-in and re-detection [S3]
The owned bin directory MUST be appended LAST to the PATH used for detection and spawn. After install or remove the system MUST refresh that PATH and force engine re-detection, so the llamacpp engine moves between `not installed` and `not running` without restart.

#### Scenario: Detected after install [auto]
- GIVEN no system `llama-server` and a completed install
- WHEN re-detection is forced
- THEN llamacpp is `not running` (not `not installed`)

#### Scenario: Reverts after remove [auto]
- GIVEN only the owned install
- WHEN it is removed and re-detection forced
- THEN llamacpp is `not installed`

### Requirement: System binary precedence [S3]
When a `llama-server` exists on the user's PATH, it MUST win over the owned install. The wizard MUST say so, and SHOULD NOT offer install when a system binary is present (remove stays available for an existing owned install).

#### Scenario: System binary wins [auto]
- GIVEN a system `llama-server` and an owned install
- WHEN the binary is resolved
- THEN the system path is used

#### Scenario: Install not offered over system binary [auto]
- GIVEN a system `llama-server` and no owned install
- WHEN the llamacpp row menu is built
- THEN no `install llama-server…` entry is shown

### Requirement: Managed offer and HF offer [S4]
After a successful install the wizard MUST offer `managed = true` and then the Hugging Face download, each as y/n; declining either MUST leave the install intact.

#### Scenario: Offers in order [auto]
- GIVEN a completed install
- WHEN the wizard finishes activation
- THEN managed is offered first, then the HF download, and `n` to both keeps the install

### Requirement: Pinned tag and newer-tag notice [S2][S3]
The installed tag MUST stay pinned; no automatic update occurs. When a newer `^b\d+$` tag with the asset exists, the system MUST show a notice and MUST NOT install it; the user re-runs the flow. The check MUST NOT block the UI and a network failure MUST be silent.

#### Scenario: Newer tag notice [auto]
- GIVEN installed `b9000` and available `b9100`
- WHEN the check completes
- THEN a notice mentions `b9100` and nothing is downloaded

#### Scenario: Offline check silent [auto]
- GIVEN the releases request fails
- WHEN the check runs
- THEN no error is shown and no notice appears

### Requirement: Remove llama-server [S3][S4]
A `remove llama-server` entry MUST exist when an owned install exists. It MUST ask y/n, delete `<data>/lazarobox/llama.cpp/` entirely (all tags and `current`), and MUST NEVER touch a system binary. A successful install SHOULD prune older tag directories after activation.

#### Scenario: Remove deletes owned only [auto]
- GIVEN an owned install and a system binary
- WHEN remove is confirmed
- THEN the owned directory is gone and the system binary is untouched

#### Scenario: Prune old tags [auto]
- GIVEN installed `b8900` and `b9000` and a new successful `b9100` install
- WHEN activation completes
- THEN `b8900` is removed, `b9000` (the immediately previous tag) is kept for manual rollback, and `current` points to `b9100`

### Requirement: Cancel and cleanup [S3]
Cancelling during download or extraction (or any failure) MUST remove partial archives and temp directories and MUST leave a previously active install untouched.

#### Scenario: Cancel mid-download [auto]
- GIVEN a download in progress
- WHEN the user cancels
- THEN the partial file is deleted and no tag directory exists

#### Scenario: Failure keeps previous install [auto]
- GIVEN an active `b9000` and a failing `b9100` install
- WHEN it fails
- THEN `current` still resolves to `b9000` and `b9100` leaves no files

### Requirement: Errors [S3][S4]
Network, rate limit (GitHub 60/h), HTTP, disk, permission and tool-missing (`tar`, `curl`) failures MUST each produce a short readable message in the wizard, never a panic, and MUST trigger cleanup.

#### Scenario: Rate limited [auto]
- GIVEN GitHub answers 403 with rate-limit headers
- WHEN resolution runs
- THEN the wizard shows a rate-limit message with no stack or raw body

#### Scenario: tar missing [auto]
- GIVEN `tar` is not on PATH
- WHEN the extract step is reached
- THEN the flow stops with a clear message and the archive is deleted

### Requirement: Real-host install [manual]
#### Scenario: Clean host end to end [manual]
- GIVEN a Linux x64 host without llama-server
- WHEN the user walks the wizard accepting every step
- THEN llamacpp goes `not installed` → `not running`, and after remove returns to `not installed`
