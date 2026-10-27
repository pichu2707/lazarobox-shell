# Tasks: Guided llama-server install

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1,410 production (tests excluded, per project rule) |
| 400-line budget risk | Low per PR (each <= ~350); High as a single PR |
| Chained PRs recommended | Yes |
| Suggested split | PR1 -> PR2 -> PR3 -> PR4 -> PR5 -> PR6 -> PR7 |
| Delivery strategy | user approved exceeding 400 when needed |
| Chain strategy | stacked-to-main |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: Low

### Assumptions (user to confirm)
- A1: successful install prunes older tags but keeps the immediately previous tag for manual rollback (spec "Prune old tags" must read "keeps previous").
- A2: newer-build notice respects `[updates] check = false`.
- A3: install not offered when a system binary exists; remove deletes the owned llama.cpp dir; Vulkan only on Linux with an ICD; missing/malformed digest aborts.
- A4: design slice 5 is split into PR5 (pure wizard) and PR6 (wiring + rendering).

### Suggested Work Units (all base = main, merged in order)

| Unit | Goal | PR | Lines | Depends |
|------|------|----|-------|---------|
| 1 | HF row ungated, llamacpp always listed | PR1 | ~60 | none (independent) |
| 2 | Pure install logic + `binary` field | PR2 | ~350 | none |
| 3 | Curl generalisation, resolve, download, verify | PR3 | ~300 | PR2 |
| 4 | Extract, activate, prune, remove, lock, PATH | PR4 | ~330 | PR2, PR3 |
| 5 | Wizard state machine (pure) | PR5 | ~200 | PR2 |
| 6 | Effects/events/menu plumbing + rendering | PR6 | ~220 | PR3, PR4, PR5 |
| 7 | Manage rows, newer notice, README | PR7 | ~150 | PR6 |

## PR1: HF row independent (config-menu "HF row independent")

- [x] 1.1 RED: `src/core/ai/picker.rs` tests: row visible when NotInstalled; no note when installed; row hidden without curl; llamacpp listed on Engines when NotInstalled. ("row hidden without curl" not testable here: the pure picker has no curl signal and the hub already handles missing curl; no behavior change.)
- [x] 1.2 GREEN: `picker.rs` (l.195-228, 252-267) list llamacpp always, ungate HF row, add note (~60). Done when `cargo test` green.
- [ ] 1.3 [manual] Download works without llama-server (spec "Download works without llama-server").

## PR2: Pure logic + binary field

- [x] 2.1 RED: `src/core/ai/install.rs` tests: `pick_release` newest tag, numeric compare (b999 vs b1000), no usable release, missing/malformed digest; `asset_name` per platform; Vulkan with/without ICD; unsupported platform.
- [x] 2.2 GREEN: create `install.rs` (`Variant`, `Platform`, `asset_name`, `Release`, `pick_release`) (~150).
- [x] 2.3 RED: `check_member` (traversal, absolute, control chars, `\`, wrong top), symlink rule (bare name ok, escaping rejected), `needed_bytes`, newer-tag compare, update-check opt-out (A2).
- [x] 2.4 GREEN: implement those in `install.rs` (~120).
- [x] 2.5 RED: `Probe::Server.binary`, `EngineState.binary`, `start_command` absolute path only when owned (chat-engines "Owned binary spawned", "System binary preferred").
- [x] 2.6 GREEN: `src/core/ai/mod.rs` + `src/runtime/ai_detect.rs` report found path (~80).

## PR3: Download and verify (needs PR2)

- [x] 3.1 RED: `src/runtime/hf/tests.rs`-style tests with local HTTP server: label in errors, 403/429 message, resolve picks newest with asset, no usable release.
- [x] 3.2 GREEN: `src/runtime/hf.rs` `Curl.label`, `github_api`/`github`, `pub(super)` helpers (~80).
- [x] 3.3 RED: `src/runtime/llama_install.rs` tests: digest match, mismatch deletes archive, missing digest aborts, insufficient space, cancel keeps `.download`, resume.
- [x] 3.4 GREEN: resolve + download + size/sha256 in `llama_install.rs` (~140).
- [x] 3.5 RED: attestation: failure aborts; `gh` absent or logged out warns and continues (fake `gh` on PATH).
- [x] 3.6 GREEN: `gh attestation verify` step (~80).

## PR4: Extract, activate, remove (needs PR2, PR3)

- [x] 4.1 RED: fixture tarballs: valid, `../` member, absolute member/symlink, `..` symlink, extra top dir, bare-name symlink ok, ownership not preserved, tar missing.
- [x] 4.2 GREEN: `tar -tzf`/`-xzf` + Rust walk in `llama_install.rs` (~130).
- [x] 4.3 RED: fake `llama-server` script: version pass/fail (failure keeps previous install, removes tag dir), layout `<tag>/`+`current`, XDG fallback, prune keeps previous tag (A1), lock refusal, stale staging sweep, remove deletes owned dir only.
- [x] 4.4 GREEN: activate (atomic symlink swap), prune, flock, remove (~120).
- [x] 4.5 RED: PATH plug-in: owned appended last, none changes nothing, forced re-detection finds/loses binary, system wins.
- [x] 4.6 GREEN: `src/runtime.rs` (l.266-270) append `current` to login PATH (~80).

## PR5: Wizard state machine (needs PR2)

- [x] 5.1 RED: `src/core/ai/picker/install.rs` tests: no step without consent, Esc backs out, copy-only row never emits an ask, variant rows, offers order (managed then HF).
- [x] 5.2 GREEN: pure `Wizard`/`Screen`, `InstallAsk` outcomes, `Level::Install` in `picker.rs` (~200).

## PR6: Plumbing and rendering (needs PR3, PR4, PR5)

- [ ] 6.1 RED: `src/app/install.rs` tests: Outcome -> `MenuCommand` -> `Effect::Install`; `AppEvent::Install{Progress,Reply}` -> `install_reply`; Done emits `Effect::Detect{force:true}`.
- [ ] 6.2 GREEN: `src/app.rs`, `src/app/install.rs`, `src/core/menu.rs`, `src/runtime.rs` worker + inbox (~140).
- [ ] 6.3 RED/GREEN: `src/ui/components/menu.rs` snapshot tests per wizard screen, then render (~80).
- [ ] 6.4 [manual] Real download, Linux x64 Vulkan, clean host (spec "Clean host end to end"). macOS arm64/x64 unverified (D12).

## PR7: Manage rows and docs (needs PR6)

- [ ] 7.1 RED: `picker.rs` tests: install entry when absent; remove entry with owned; none with system binary; newer notice once per run, offline silent, silent when `[updates] check = false`.
- [ ] 7.2 GREEN: manage rows + `CheckNewer` wiring (~110).
- [ ] 7.3 `README.md` (Spanish) exception note, `core/ai/mod.rs` doc l.100-102 (~40).
