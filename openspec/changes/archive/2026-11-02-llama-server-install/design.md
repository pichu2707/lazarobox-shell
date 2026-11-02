# Design: Guided llama-server install

## Technical Approach

Approach A of the proposal. Pure logic (asset naming, release selection, digest/member rules,
disk estimate, update notice, wizard state machine) lives in `src/core/ai/`; IO (GitHub API,
download, `gh`, `tar`, filesystem, lock) lives in `src/runtime/llama_install.rs`. The flow
follows the existing HF pipeline exactly: picker `Outcome` -> `MenuCommand` -> `Effect` ->
runtime worker -> `AppEvent` -> picker (as `src/app/hub.rs:31-60`, `src/runtime/hf.rs:717-846`).
Verified archive facts (engram 3384): single top dir `llama-<tag>/`, bare-name relative
symlinks, RUNPATH `$ORIGIN`, launcher + `libllama-server-impl.so` -> keep the whole dir, no
`LD_LIBRARY_PATH`.

## Architecture Decisions

| # | Decision | Alternatives rejected | Rationale |
|---|----------|-----------------------|-----------|
| D1 | Generalise `Curl` (`runtime/hf.rs:46-212`): add `label: &'static str` (error prefix, today hardcoded at l.176/202/209/266) and constructors `Curl::github_api(path)` (`https://api.github.com`) and `Curl::github(path)` (`https://github.com`); make `get`, `download_args`, `fetch`, `sha256`, `free_space` `pub(super)`. Download URL is built by us: `/ggml-org/llama.cpp/releases/download/<tag>/<asset>`, never taken from JSON. | Copy curl code; a full-URL download API | Keeps `-q`, https-only incl. redirects (l.82-84), resume (l.127), process-group kill (l.152-187) in one place; base stays injectable for tests (`hf/tests.rs:79`). |
| D2 | Release = newest `^b\d+$` tag from `GET /repos/ggml-org/llama.cpp/releases?per_page=10` that has the wanted asset with a `sha256:` digest. `/releases/latest` never used. | `/releases/latest`; `nightly-tag.txt` | Verified: latest is `v0.7.0` without binaries. One API call per install. |
| D3 | Verify inside the download step, not as optional steps: size + sha256 in Rust (`keep()` pattern, l.509-535) then `gh attestation verify <archive> --repo ggml-org/llama.cpp` when `gh` is on PATH and `gh auth status` succeeds; mismatch/attestation failure deletes the archive and stops. `gh` absent or logged out: warning row, continue. | y/n per verification; parse `sha256sum` | Declining verification must not be possible; tool output is localized. |
| D4 | Extraction shells out to `tar` (no new crate): (1) `tar -tzf` with `LC_ALL=C`, absolute `tar` via `find_on_path`; every name must start with `llama-<tag>/`, no absolute, no `..` component, no control chars or `\`; cap 10k entries. (2) `tar -xzf A -C <fresh staging> --no-same-owner`. (3) Walk the result in Rust with `symlink_metadata`: only files/dirs/symlinks; `read_link` targets must be bare names existing in the same dir. | `tar -tzvf` parsing; `tar`+`flate2` crates | Verbose listing differs GNU vs BSD and is locale/date dependent; flags used are common to GNU tar and bsdtar; both refuse `..` and write symlinks safely by default; post-walk validation reads exact targets without parsing. Crate would be cleaner but adds 2 deps for one feature. |
| D5 | Layout `<data_home>/lazarobox/llama.cpp/{<tag>/, current -> <tag>, .download/<asset>, .staging-<pid>/, .lock}` (`core/desktop.rs:126` `data_home`). Activation: rename staging top dir to `<tag>/`, run `<tag>/llama-server --version` (5 s, capped output), create `current.new` symlink, `rename` over `current` (atomic), prune other tag dirs, delete archive. `flock(LOCK_EX|LOCK_NB)` on `.lock` for install/remove. Cleanup: any failure removes `.staging-<pid>/` and a just-renamed `<tag>/`; stale `.staging-*` are swept under the lock at each install start; digest/attestation failure deletes the archive; cancel keeps it for resume. | Install in `~/.local/bin`; copy binary only | Dir must stay intact for `$ORIGIN`; atomic swap; second lazarobox gets a clear refusal. |
| D6 | PATH plug-in: append `<...>/llama.cpp/current` ONCE at startup to the login PATH before `CliEnv::with_path` (`runtime.rs:266-270`), last entry. No runtime refresh. | Refresh `CliEnv.path`/`Managed.path` snapshots after install; separate llama-only PATH | `find_on_path` (`ai_detect.rs:75`) checks the file at lookup time; detection (`:585`) and `Managed::spawn` (`chat/managed.rs:413-417`) look up on each call, so a stable entry whose content appears/disappears needs no snapshot refresh. Last = system binary wins. After install/remove the app emits forced re-detection (existing `Outcome::Refresh` -> `spawn_detection(force=true)`, `runtime.rs:403-412`). |
| D7 | `Probe::Server.on_path: bool` -> `binary: Option<PathBuf>` (`core/ai/mod.rs:225-229`); `EngineState` gains `binary: Option<PathBuf>`; `start_command(id, port, binary: Option<&Path>)` (`mod.rs:103`) prints the absolute path (single-quoted when needed) only when the binary is inside the owned dir. | Separate owned flag in Detection | One fact serves start hint, "owned vs system" label and remove gating. |
| D8 | UI: llamacpp is always listed on the Engines level once detection is known (today hidden when `NotInstalled`, `picker.rs:195-228`); its Models level shows the HF row regardless (`picker.rs:252-267`) plus `install llama-server…` (not installed), or `llama-server <tag> (lazarobox) · remove…` / `newer <tag> available · install…` when owned. Wizard = new `Level::Install(Wizard)` in `src/core/ai/picker/install.rs`, mirroring `Level::Hub` (`picker.rs:615-626`, `picker/hub.rs:90-123`). | New AI-tab menu row; global `InputMode::Confirm` modal | Keeps everything llama on the llamacpp row; reuses frames, Esc/back, cancel prompt. |
| D9 | Steps with y/n gates: Plan (variant rows CPU, Vulkan when an ICD file exists in `/usr/share/vulkan/icd.d` or `/etc/vulkan/icd.d`; macOS: Metal build; plus "I'll do it myself" copy-only row) -> Resolve -> Download+verify (progress, cancel keeps `.download` for resume) -> Install (extract, validate, version, activate) -> "Download a model from Hugging Face?" (opens `Level::Hub`). Managed is offered by the existing local-model commit (`config.rs:518 manage`), not a new step. | Separate managed step | `managed` requires a model file. |
| D10 | Update notice only when the user opens the llamacpp Models level with an owned install (one API call, at most once per run). No background network. | Startup check | No hidden network calls, rate limit friendly. |
| D11 | Rate limit: HTTP 403/429 -> "GitHub rate limit reached (60 requests/hour without a token); try again later". No GitHub token read. | Read `GITHUB_TOKEN` | Few calls; no new secret handling. |
| D12 | macOS arm64/x64 in v1 (asset `macos-<arch>` only differs in naming); `--version` gate catches an unknown rpath; failure removes the tag dir. | Follow-up PR | Cheap; same code path. |

## Data Flow

    picker/install.rs --Outcome::Install(InstallAsk)--> menu --> App --Effect::Install-->
    runtime/llama_install.rs (Installer worker, curl/gh/tar) --AppEvent::Install{Progress,Reply}-->
    App --> menu.install_reply() --> Wizard screen; on Done: Effect::Detect{force:true}

## File Changes

| File | Action | Description |
|------|--------|-------------|
| `src/core/ai/picker.rs` | Modify | llamacpp listed when not installed; HF row ungated; install/manage rows |
| `src/core/ai/install.rs` | Create | Platform/asset names, release selection, digest, member/symlink rules, disk estimate, newer-tag compare |
| `src/core/ai/picker/install.rs` | Create | Wizard `Screen` state machine (pure) |
| `src/core/ai/mod.rs` | Modify | `Probe`/`EngineState.binary`, `start_command`, doc l.100-102 exception |
| `src/runtime/hf.rs` | Modify | `Curl` label + GitHub constructors, shared helpers |
| `src/runtime/llama_install.rs` | Create | Installer worker: resolve, download, verify, gh, tar, activate, remove, lock |
| `src/runtime/ai_detect.rs` | Modify | Report found binary path |
| `src/runtime.rs` | Modify | PATH append, installer wiring, inbox channel |
| `src/app.rs`, `src/app/install.rs`, `src/core/menu.rs` | Modify/Create | Effects, events, commands |
| `src/ui/components/menu.rs` | Modify | Render wizard screens |
| `README.md` | Modify | Documented exception (Spanish) |

## Interfaces / Contracts

```rust
// core/ai/install.rs
pub enum Variant { Cpu, Vulkan, Metal }
pub struct Platform { pub os: Os, pub arch: Arch }
pub fn asset_name(tag: &str, p: Platform, v: Variant) -> Option<String>;
pub struct Release { pub tag: String, pub asset: String, pub size: u64, pub sha256: String }
pub fn pick_release(json: &str, asset_of: impl Fn(&str) -> Option<String>) -> Result<Release, String>;
pub fn check_member(name: &str, top: &str) -> Result<(), String>;
pub fn needed_bytes(archive: u64, on_disk: u64) -> u64; // archive*5 - partial
pub enum InstallAsk { Resolve(Variant), Download(Release), Install(Release), Remove, CheckNewer }
```

## Testing Strategy

| Layer | What | Approach |
|-------|------|----------|
| Unit | Tag/asset selection, digest, member rules, wizard transitions, rows | Pure fns; JSON fixtures trimmed from real API reply |
| Integration | API/download/resume/digest mismatch/403 | Real curl vs local HTTP server (existing `hf/tests.rs` pattern), `https_only: false` |
| Integration | Extract/validate | Fixture tarballs built in tests with `tar -czf` (valid, absolute symlink, `..` symlink, extra top dir); hand-written ustar bytes + `gzip` for `../` names; fake `llama-server` script for `--version`; runs on ubuntu and macOS CI |
| Integration | PATH plug-in | Temp data home: install fake -> forced detection -> NotRunning; system dir earlier on PATH wins; remove -> NotInstalled |

## Migration / Rollout

No migration. Chained slices (production lines only, tests excluded):

| Slice | Content | Est. lines |
|-------|---------|-----------|
| 1 | D8 part: llamacpp listed when not installed, HF row ungated, note | ~60 |
| 2 | `core/ai/install.rs` pure logic + D7 `binary` field | ~350 |
| 3 | D1 Curl generalisation, resolve, download, digest, gh attestation | ~300 |
| 4 | tar list/extract/walk, activate, prune, remove, lock, D6 PATH | ~330 |
| 5 | Wizard state machine, menu/app plumbing, rendering | ~420 |
| 6 | Manage rows (remove, newer notice), README + doc comment | ~150 |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Unsandboxed upstream binary | Med | Consent gates, digest, attestation when possible |
| `gh` logged out / no attestation for an asset | Med | `gh auth status` first; logged out = warn |
| glibc too old (Ubuntu builds) | Med | `--version` before `current` swap; tag dir removed on failure |
| macOS archive layout unverified | Med | Same `--version` gate; real check in verify phase |
| Extraction size larger than 5x estimate | Low | Post-extract failure cleans staging; estimate tunable |
| Appended PATH entry visible to all child tools | Low | Last entry, only llama.cpp binaries |

## Open Questions

- [ ] Prune old tags on install (chosen) vs keep one previous for manual rollback.
- [ ] Should the update notice also respect `[updates] check = false`? (proposed: yes).
