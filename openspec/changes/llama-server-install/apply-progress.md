# Apply progress: llama-server-install

## PR1 (branch feat/llama-server-install-pr1) - done except manual task

Mode: Strict TDD. Delivery: stacked-to-main.

- [x] 1.1 RED, [x] 1.2 GREEN
- [ ] 1.3 [manual] pending for the user (download without llama-server)

### TDD Cycle Evidence

| Task | Test file | RED | GREEN | TRIANGULATE | REFACTOR |
|------|-----------|-----|-------|-------------|----------|
| 1.1/1.2 HF row ungated + note | src/core/ai/picker.rs (tests) | compile failure on missing HF_NEEDS_SERVER, then 4 failing asserts | cargo test 6096 passed | NotInstalled and missing-from-detection; installed (NotRunning, Available); pending detection | none needed |
| 1.1/1.2 llamacpp on Engines level | src/core/ai/picker.rs (tests) | failed before `is_missing_llama_server` | green | not-installed vs pending detection vs already assigned (listed once) | none needed |

Existing tests/snapshots updated deliberately (each diff = one added `llamacpp not installed` row, or the HF row now present with a note): hub_tests, disabled_tests, picker_tests, reanchor_tests, 3 insta snapshots.

### Deviations
- Note is a row mark (`needs llama-server to run`), not separate wrapped note rows.
- Scenario "Row still hidden without curl": no test; the picker is pure and has no curl signal.

### PR1 review fixes

- WARNING 2/1: the Models level of a NotInstalled llamacpp now says `llama-server not installed: install it to use these models` instead of `engine not detected` (RED: `an_unmanaged_or_absent_llama_server_keeps_its_rows` pin and a new menu test failed first; GREEN after the new branch in `model_rows`). No version is promised for the install entry.
- WARNING 3: menu-level test `a_not_installed_llamacpp_explains_itself_and_leaves_the_draft_alone` (Enter on the llamacpp row, cursor on the HF row, `s` keeps `ai_draft()`, Esc returns to Engines) plus snapshot `snapshot_models_level_of_llamacpp_without_llama_server` at MIN_WIDTH.
- SUGGESTION 2: `reanchor_tests` also asserts the row count (4).
- SUGGESTION 1: the real detector always emits llamacpp (`reduce` walks the whole CATALOG), so the missing-from-detection case is test-only; the comment says so.
- Known limitation, not fixed: at MIN_WIDTH (24) the explanation row, the footer and the HF row (its mark hides the label) are clipped with an ellipsis; at the 62-wide popup the text fits.
- Pin `hub_tests.rs:61` (`src/core/ai/picker/hub_tests.rs`) pins only the HF mark, so it needed no change.
## PR2: Pure logic + binary field (branch feat/llama-server-install-pr2)

Status: 6/6 tasks done (2.1-2.6).

- `src/core/ai/install.rs` (new): `Variant`, `Arch`, `Platform`, `asset_name`, `variants`,
  `Release`, `pick_release` (numeric tag compare, digest required on the chosen release),
  `check_member`, `check_link_target`, `needed_bytes`, `is_newer`, `should_check_newer`
  (A2), `owned_root`, `is_owned`.
- `Probe::Server.on_path: bool` -> `binary: Option<PathBuf>`; `EngineState.binary`;
  `start_command(id, port, binary)` prints the absolute (shell-quoted) path only when
  `install::is_owned`. `ai_detect::probe` reports the found path. Picker passes the
  detected binary to the not-running hint.

Deviations / notes for later PRs:
- `Os` is reused from `core::ai::jail`; `Arch` is new in `install.rs`.
- `is_owned` was first a substring match on components; replaced in the review fixes below.
- Design D5 renames the staging top dir to `<tag>/` (so `current/llama-server` is on PATH);
  the spec layout scenario says `<tag>/llama-<tag>/llama-server`. PR4 should follow D5 and
  fix the scenario text.
- `pick_release` aborts (Err) when the newest release holding the asset has a missing or
  malformed digest, instead of silently taking an older release.
- Tests and implementation for 2.1-2.4 were written together (RED not observed separately).

### PR2 review fixes

- `is_owned(binary, data_home)`: no `..`/`.` component and `starts_with(owned_root(data_home))`
  (the root itself is not a binary). Spoofs `/opt/lazarobox/llama.cpp/...` and `..` climbs are
  rejected. `Detection.data_home: Option<PathBuf>` (set in `detect_all` from `env.data.parent()`)
  feeds the picker; `start_command(id, port, binary, data_home)` (signature change). No data
  home known means nothing is owned (bare `llama-server`).
- `pick_release`: a missing, zero or non-numeric asset `size` is an error like a bad digest
  (never falls back to an older release).
- `check_hardlink_target(target, top)` (new): tar hardlink targets are archive-root-relative, so
  it reuses `check_member`. `check_link_target` stays for symlinks (bare sibling names).
- `check_member` fails closed on empty (`//`) and `.` components (one trailing `/` allowed).
- `start_command` quotes only an owned path; non-UTF-8 or control-character paths fall back to
  bare `llama-server`.
- Tests: spoofs, 65/64+junk/empty/short digests, prerelease selected, tag overflow skipped,
  `//`, `./`, NUL members, `$`/backtick/control-char/non-UTF-8 paths; `one()` no longer forces
  `binary: None` (binary assertions restored via `state_on_path`).
- Strict TDD: RED observed (13 failing tests) before the implementation.
## PR3: Download and verify (branch feat/llama-server-install-pr3, from PR2 9ec1f53)

Status: 6/6 tasks done (3.1-3.6). Mode: Strict TDD.

Commits:
- `refactor(hf): label curl errors and add GitHub constructors` (3.1, 3.2)
- `feat(ai): resolve and download a verified llama.cpp archive` (3.3, 3.4)
- `feat(ai): verify the archive attestation with gh when available` (3.5, 3.6)

Delivered:
- `Curl.label` ("Hugging Face" / "GitHub") names the service in errors; Hub texts unchanged
  (`NO_CURL` kept). `Curl::github_api(path)`, `Curl::github(path)` (https only, no token),
  `#[cfg(test)] Curl::local(base)`. `get`, `spawn`, `download_args`, `fetch`, `finish`,
  `error_line`, `sha256`, `file_len`, `has_size` are `pub(super)`. `hf::tests` helpers
  (`serve`, `file_server`, `reply`, `ok`, `header`, `sha`, `no_cancel`, `curl_available`) are
  `pub(in crate::runtime)` so the installer tests reuse the fake server.
- `src/runtime/llama_install.rs`: `resolve(curl, platform, variant)` (releases?per_page=10,
  403/429 -> `RATE_LIMITED`), `archive_path(root, release)` (`<root>/.download/<asset>`),
  `download(curl, root, release, progress, &mut cancel) -> End` (name safety, digest
  well-formed, space check with `needed_bytes`, resume, size + sha256 in Rust, a wrong archive
  is deleted), `download_verified(curl, gh_path, root, release, progress, &mut cancel) ->
  Fetched {Done{archive, warning}, Cancelled, Failed}` (`gh auth status`, then
  `gh attestation verify <archive> --repo ggml-org/llama.cpp`; `gh` absent -> `NO_GH`
  warning, logged out -> `GH_LOGGED_OUT` warning; failure deletes the archive; 60 s limit;
  cancel keeps the archive).

TDD Cycle Evidence:

| Task | RED (observed) | GREEN | Triangulation / refactor |
|------|----------------|-------|--------------------------|
| 3.1/3.2 | `cargo test --lib runtime::hf`: E0560 no field `label`, E0599 no `github_api`/`github`/`local` | 33 passed (30 baseline + 3) | Hub label asserted unchanged (`NO_CURL`, `hf::UNREADABLE`) |
| 3.3/3.4 | `cargo test --lib runtime::llama_install`: unresolved `resolve`, `download`, `Platform`... | 15 passed | digest mismatch, size mismatch, missing/odd digest, unsafe names, no space, cancel, resume, complete archive, oversized partial, rate limit 403+429, 500, no usable release. Mutation check: skipping the mismatch branch fails `a_digest_mismatch_deletes_the_archive` |
| 3.5/3.6 | unresolved `Fetched`, `download_verified`, `NO_GH`, `GH_LOGGED_OUT`, `attest_within`; `download` signature mismatch | 22 passed (5 consecutive runs green) | pass, fail aborts, gh absent, logged out, mismatch never reaches gh, cancel stops gh, hang times out |

Deviations / notes for later PRs:
- Spec "Cancel mid-download: partial file deleted" contradicts D5 (cancel keeps `.download`
  for resume) and task 3.3; D5 followed. The spec scenario text should be fixed at archive.
- `download` takes `&mut oneshot::Receiver<()>` so `download_verified` can reuse one cancel
  for the digest step and gh.
- `gh` is looked up with `desktop::which` on the PATH passed in (the login PATH).
  PR4/PR6 must pass the owned root (`install::owned_root(data_home)`) as `root`; the
  `.download` dir lives inside it, so `remove` deletes it too.
- Unknown size (0) skips the size check and resume shortcut; the digest still guards.
