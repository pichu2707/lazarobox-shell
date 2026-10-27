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

## PR3 review fixes

Commit `fix(ai): harden the llama.cpp download and attestation checks` (Strict TDD, RED
observed: 5 unresolved items at compile time, then 29 passed x10 consecutive runs).

- gh attestation timeout no longer deletes the digest-verified archive:
  `Attestation::Unchecked` -> `Fetched::Failed("could not check the attestation: timed out")`
  (`UNCHECKED_TIMEOUT`), archive kept. Only a nonzero `gh attestation verify` deletes it.
- `gh auth status --hostname github.com`; logged-out is told from other failures by the
  stderr line ("not logged in" / "gh auth login") -> `GH_LOGGED_OUT`; anything else or a
  timeout -> a "gh could not check its login (...)" warning (`Skipped`). gh children clear
  `GH_HOST`, `GH_ENTERPRISE_TOKEN`, `GH_REPO` (`gh_command`).
- GitHub download only: `--max-filesize <size>` (omitted when size 0), `--max-redirs 5`,
  `--speed-limit 1 --speed-time 60` via `Curl::download_args_within(path, out, Some(size))`;
  `fetch` takes `bound: Option<u64>` (Hub passes `None`; its args are unchanged).
  `download_args` is now `#[cfg(test)]`.
- `.download` created 0700; a symlink at `.download` or at the archive is refused before any
  read/remove/write (`refuse_links`, `create_dir`).
- gh gets the archive as an absolute path (`std::path::absolute`).
- Flake: the cancel test polls the fake gh log for `attestation verify`; ETXTBSY retry is
  10 x 50 ms.

Notes for later PRs:
- PR4 MUST re-hash (or re-check size + sha256 of) the archive right before extraction: the
  digest is checked at download, and `.download` is only private to the user, not immutable.
- PR4/PR6: `Fetched::Failed` can now leave the archive on disk (timeout); a retry
  re-verifies it without a download.
- Follow-up: `--signer-workflow ggml-org/llama.cpp/.github/workflows/release.yml` is NOT
  added: that path is not confirmed in the repo docs or notes. Verify upstream's release
  workflow path before pinning it.
## PR4: Extract, activate, remove, PATH (tasks 4.1-4.6)

Commits (on feat/llama-server-install-pr3):
- feat(ai): append the owned llama.cpp folder last on the PATH
- feat(ai): unpack, activate and remove the llama.cpp install

Implemented:
- `core/ai/install.rs`: `current_dir`, `with_owned_path(path, data_home)` (appended once, last).
  `runtime.rs` applies it to the login PATH before `CliEnv::with_path` (D6).
- `runtime/llama_install/unpack.rs`: `find_tar`, `extract(tar, archive, staging, top)`: `tar -tzf`
  (LC_ALL=C, no TAR_OPTIONS) judged by `check_member` (cap 10k), `tar -tzvf --numeric-owner`
  first char (only `- d l h`; `h` target via `check_hardlink_target`), `tar -xzf -C --no-same-owner`,
  then a Rust walk (`symlink_metadata`): one top dir, files/dirs/bare-name existing symlinks,
  no setuid/setgid, and an inode/nlink census that refuses any file linked outside the tree.
- `runtime/llama_install/activate.rs`: `install(root, release, archive, path)`, `remove(binary,
  data_home)`, `flock` on `.lock` (LOCKED), stale `.staging-*` sweep, version check
  (`run_capped`, 5 s), atomic `current.new` rename, prune keeping the previous tag (A1).
- PR3 review requirement (archive swap): install refuses a symlink at the archive path, opens it
  `O_NOFOLLOW`, copies it into the 0700 staging dir while hashing exactly the copied bytes against
  `release.sha256`/`size`, and extracts only the copy.

TDD evidence:

| Task | RED (observed) | GREEN |
|------|----------------|-------|
| 4.5/4.6 | with a stub `with_owned_path` returning the input: 2 tests FAILED | 28 install tests pass |
| 4.1/4.2 | `cargo test --lib llama_install::unpack`: E0425 `extract`, `find_tar`, `NO_TAR`, `MAX_ENTRIES` | 15 passed |
| 4.3/4.4 | process deviation: `activate.rs` was written before its tests; compensated with mutation checks: digest check off -> swap test FAILS; prune off -> prune test FAILS; version check off -> 3 tests FAIL; symlink check off -> symlink test FAILS | 16 passed |

Notes:
- Hardlinks: the verbose listing is parsed only for its first character (numeric owner keeps the
  fields digits), but a crafted name could still fool " link to " parsing, so the walk's nlink
  census is the authoritative defence; both stay.
- GNU tar run as a normal user drops setuid bits itself; the walk refuses them when root keeps them.
- Hostile members (`..`, absolute, escaping symlink, outside hardlink, device) are built with a
  hand-written ustar writer in the tests, so no root is needed. Tests need `tar` and `gzip`.
- A free `flock` can look taken while another thread's fork holds the fd copy before exec:
  `lock` looks 5 times 25 ms apart before reporting LOCKED (found as a flaky test).
- Spec scenario "Layout after install" fixed to D5 (`<tag>/llama-server`).
- For PR6: call `activate::install(owned_root(data_home), release, archive, login_path)` after
  `download_verified`, then emit `Detect{force:true}`; `activate::remove(binary, data_home)`.

### PR4 review fixes

Commit: `fix(ai): make llama.cpp unpack, activate and remove robust against hostile modes and blocking`.

- Closed folders (mode 000/0500): `make_owner_rwx` (iterative, `symlink_metadata`, never follows a
  link) runs after `tar -x`, before the walk. `remove_tree` (same chmod, then `remove_dir_all`,
  error returned) now deletes staging, tags, prune and remove. A failed staging cleanup is appended
  to a failing install's message; after a successful install it is left for the next sweep.
- Blocking work: `lock`, `sweep_staging`, staging creation, staging removal, tag undo and `prune`
  run in `spawn_blocking` (`blocking` helper). `remove` is now `async` and runs whole on the
  blocking pool (API change: `remove(binary, data_home).await`).
- Listing: a member name with `" link to "` or `" -> "` is refused outright; a hardlink target
  with a second `" link to "` is refused; symlink targets are now judged from the verbose listing
  before anything is written (GNU tar itself refused a write through an absolute symlink dir, but
  our checks did not; now they do). A newline in a name is refused by `check_member` (GNU prints
  it escaped as a backslash). The nlink census stays.
- Same-tag reinstall: the tag `current` points at is refused with "<tag> is already installed".
  Reinstalling the kept previous tag still works (old folder moved aside, put back on failure).
- `remove`: a symlink or dangling-symlink root is removed as a link before any lock; contents go
  first, then `.lock`, then the root (a waiter that lost the lock fails on the missing folder).
- `with_owned_path`: verified, the premise was only partly true: only the lone empty entry of a
  fully empty PATH is dropped (correct); empty and `.` entries in a longer PATH were already kept.
  Comment added and a test that pins it (no RED possible: it passed on the unchanged code).
- Cross-process lock: tested with `flock(1)` holding `.lock` from a child process group (skips with
  a message when `flock` is absent).

Mutation/RED evidence (each test fails without its fix): chmod off -> 2 closed-folder tests fail;
`remove_tree` chmod off -> 2 fail; link-like name check off -> 1 fails; current-tag guard off -> 1
fails; lock-before-symlink order -> dangling root test (+3) fail. Symlink-then-write test was RED
before the listing check (`tar failed`, not our refusal).

Risks:
- bsdtar (macOS) is UNVERIFIED: its listing format (`-tzv` columns, raw newlines in names, `->`
  and `link to` separators) was not run; only GNU tar 1.35. A newline in a name is only caught on
  GNU (escaped); on bsdtar the fragments are judged as separate names.
- Cleanup of a failed staging after a successful install is silent (swept next time).
