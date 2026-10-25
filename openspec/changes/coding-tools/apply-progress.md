# Apply progress: coding-tools

Mode: Strict TDD. Runner: `cargo test`. Delivery: ask-on-risk, feature-branch-chain.

## Batch 1: slice T (toggle plumbing, PR 3, branch feat/coding-tools-t, base feat/coding-tools)

| Task | Status | Commit |
|------|--------|--------|
| T.1 `[ai.tools]` booleans | done | feat(config): read the five coding-tools family toggles |
| T.2 owned keys in `ConfigEdit` | done | feat(config): own the family toggle keys in the comment-preserving edit |
| T.3 `FamilySet` from config into `ChatRequest.families` | done | feat(app): offer the families of the saved toggles to each chat turn |
| T.4 `max_rounds(set)` 8 / 24 | done | feat(chat): allow 24 Ollama rounds per turn when a coding family is on |
| T.5 five menu rows + caution line | done | feat(menu): add the five family toggle rows with a caution line |
| T.6 omit `tools` on an empty registry | done | fix(chat): omit the Ollama tools key when the registry is empty |

## TDD cycle evidence

| Task | RED (failing first) | GREEN | Triangulation | REFACTOR |
|------|---------------------|-------|---------------|----------|
| T.1 | `family_toggles_*`, wrong-type, notice order, master-switch tests did not compile (`ToolToggle`, `toggle`) | `ToolToggle`, `AiConfig::toggle/set_toggle`, `read_toggles` | default/on/wrong type/non-table/git independence/order of problems | none needed |
| T.2 | `ConfigEdit::toggle` missing; round-trip, comment-preservation, table-collision tests | `ConfigEdit::toggle`, `is_flag` covers the five keys | create, in place with comment, all five, KeyIsTable | none needed |
| T.3 | `AiConfig::families` missing; app test saw LEGACY for Search+GitWrite | `families()`, `offer_tools` sets `request.families` | none, each toggle, master off, app turn with 0 and 2 toggles | none needed |
| T.4 | `max_rounds` missing; loop test saw 8 bodies instead of 24 | `max_rounds`, `CODING_ROUNDS`, loop uses `limit` | pure fn over all 5 families + LEGACY/EMPTY, real loop at 24 | none needed |
| T.5 | `family_tests.rs` (order, kinds, nav, toggle keys, Enter repeat, edits, caution) did not compile; ~30 menu tests/snapshots failed on the new rows | `ItemKind::Family`, rows, `toggle_family`, `changed_toggles`, `caution`, reserved caution row | five rows x six keys, edit order, caution only on Write/Exec/GitWrite, size constant | tests re-pinned deliberately (see below) |
| T.6 | `an_empty_registry_leaves_the_tools_key_out_of_the_round_body` failed | key set only when the list is non-empty | EMPTY (absent) vs LEGACY (present) | none needed |

## T.5 churn (deliberate, reviewed)

- AI tab grows 11 -> 16 items; `VIEW_BODY` is now bound by the AI tab plus one reserved caution row (17), `MENU_BODY` 18, popup 22 rows high (was 16). The full popup needs 21 rows of terminal (was 15).
- 25 menu snapshots changed by pure blank-row/row additions (`git diff -U0 -- '*.snap'` shows no removed text except the two full-frame snapshots where the taller popup covers more of the panes). One new snapshot: `snapshot_exec_row_selected_shows_the_caution`.
- `cargo-insta` is not installed: snapshots were regenerated with `INSTA_UPDATE=always` and reviewed through `git diff`.

## Deviations

- The Exec row does not show the "needs Linux sandbox" reason: the jail check is not available to the menu yet, so it stays for X-d2 as the task allows.
- Row labels: `Search`, `Write`, `Exec`, `Git`, `Git write`.
- `AiConfig::families()` returns LEGACY when the master `[ai.tools] enabled` is off (spec: a family is active only when enabled is true).
- The popup grew by 6 rows; a terminal under 21 rows now gets the one-line fallback for the menu.

## Checks

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (all green), `cargo +1.88 check --all-targets`.

## Batch 2: slice M1-a (glob core, PR 4, branch feat/coding-tools-m1a, base feat/coding-tools-t)

| Task | Status | Commit |
|------|--------|--------|
| M1a.1 glob -> anchored regex | done | feat(tools): translate a glob pattern into an anchored regex |
| M1a.2 `Call::Glob` args through the ArgDef table, Search gate | done | feat(tools): parse the glob call through the registry under the Search family |
| M1a.3 ordering, 100-match cap, truncation text | done | feat(tools): render glob results sorted and capped |

## TDD cycle evidence (M1-a)

| Task | RED | GREEN | Triangulation | REFACTOR |
|------|-----|-------|---------------|----------|
| M1a.1 | 7 of 8 `glob::tests` failed against a stubbed `translate` | `Glob::new`, `translate`, class parser | `**` shapes, `*`/`?`/classes, anchoring, escapes, 8 refusal kinds | none needed |
| M1a.2 | `Call::Glob` and `MAX_PATTERN_BYTES` did not exist (compile failure) | `Call::Glob`, `glob` `ToolDef` (pattern 512 B, optional path), `parse_built_in` arm | offered/parsed only with Search, parse order, caps, no-approval in `ask_for` | none needed |
| M1a.3 | `render` did not exist (compile failure) | `render` | sorted, dedup, exact 100 vs 101, cap keeps smallest names, bounded walk, both markers | none needed |

## Deviations / notes (M1-a)

- `regex` was only a transitive dependency; added `regex = "1"` as the design says (no new crate in Cargo.lock beyond the direct edge).
- M1a.1 implementation was first written before its tests; RED was then proven by stubbing `translate` and running the table. Noted honestly.
- Pattern rules beyond the task text: `{}` and `\` are refused (`Unsupported`) so a model gets a clear error instead of silent no-match; `..` segments, NUL, leading `/` are refused; a leading `./` is dropped; `]` first in a class is literal; classes never match `/`.
- Two exhaustive matches needed an arm for the new `Call::Glob`: `approval::ask_for` (never asks, per spec) and `fs::execute` (returns `NotReadTool` until M1-c wires it). `host.rs` untouched.
- Not here (M1-b/c): the walk, skip list, secret omission marker (`OMITTED_MARKER` stays in the runtime), sub-directory resolution, staged view marker.
- Risk: between this PR and M1-c, a Search-on chat offers `glob` but `fs::execute` refuses it. The chain merges as 0.19, so only intermediate branches are affected.

## Checks (M1-a)

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (all green), `cargo +1.88 check --all-targets`.

## Batch 3: slice M1-b (grep, paging, `.git/config` deny, PR 5, branch feat/coding-tools-m1b, base feat/coding-tools-m1a)

| Task | Status | Commit |
|------|--------|--------|
| M1b.1 `read_file{start_line,max_lines}` under Search, paging in `fs.rs` | done | feat(tools): page read_file by line window under the Search family |
| M1b.2 refuse reads of `config` / `config.worktree` under `.git` | done | feat(tools): refuse reads of a git config file |
| M1b.3 `Call::Grep`, `ToolDef grep`, `search::grep` over the budgeted walk | done | feat(tools): grep the root with a budgeted walk, redacted and capped |

## TDD cycle evidence (M1-b)

| Task | RED | GREEN | Triangulation | REFACTOR |
|------|-----|-------|---------------|----------|
| M1b.1 | `READ_MAX_LINES` missing (compile failure) for the schema/parse tests; fs window tests failed to compile (`MORE_MARKER_PREFIX`) | `ArgDef`s gated by Search, `window()`, `read_window`, `decode`, `cap` | schema off vs on, defaults, clamps, wrong type; window, last window (no marker), past end, edge line 11, redaction, read-cap cut, binary and secret still refused | `read_head`/`decode`/`cap` extracted from the old `read_file` body; the no-window tests stayed green |
| M1b.2 | fs test read `.git/config` successfully (`unwrap_err` on Ok) | `paths::is_git_config`, checked on the real path in `read_file` | config, config.worktree, nested modules, symlink alias, vs HEAD, `src/config`, `.github/config`, `config.bak`, `config/x`, case | none needed |
| M1b.3 | `Call::Grep` missing; `search::grep` and its consts missing (22 compile errors) | `Call::Grep`, `grep` ToolDef, `search::grep`, `Failure::BadPattern`, `Budget::new` | matches+lines, sub-dir and escapes, 100 vs exactly 100, 20 KiB, long line, bad/long/look-around pattern, secrets/binary/non-UTF-8/skip dirs, 1 MiB edge, redaction without oracle, links, pathological, cancel, HOME unset | `Budget::new` shared with `execute_with` |

## Deviations / notes (M1-b)

- `Lines` and `Call::ReadFile.lines` already existed from P0; M1-b only fills them. Defaults: `start_line` 1, `max_lines` 2000 (`READ_MAX_LINES`, also the clamp). A window is also bounded by the 64 KiB read cap (cut text ends with `READ_MARKER`, which then replaces the "more lines" marker).
- Paging scans at most 1 MiB (`PAGE_SCAN`), cleans and redacts that text as a whole BEFORE cutting lines (a multi-line secret is seen whole), so line numbers count lines of the cleaned text. Past the scan limit: an "empty window ... larger than the scan limit" note.
- `.git/config` deny is `paths::is_git_config`, applied on the real path in `read_file` only (symlink aliases are refused). It is not in `judge_rest`, so `list_dir` and the exec tools are unchanged.
- grep matches the pattern against the cleaned+redacted text (no oracle on secrets), skips symlinks (never followed), skips `target node_modules .git dist build .venv __pycache__` dirs, secret names (adds `OMITTED_MARKER`), non-regular and hard-linked files, files over 1 MiB, binaries and non-UTF-8. Lines are cut to 200 chars. Walk depth, visited and time reuse `Limits`. `Failure::BadPattern` is new.
- Pattern cap 512 B is enforced twice: in the registry (`TooLong`) and in `search::grep`. Compiled size limit 256 KiB; the `regex` crate gives the linear-time guarantee.
- `fs::execute` still returns `NotReadTool` for `Call::Grep` and `Call::Glob`; `search::grep` is called by tests only. `approval::ask_for` returns `None` for `Call::Grep` as a placeholder. The spec rule (ask on Cloud, free on Local, via `ApprovalAsk::Search`) is M1c.1, which MUST replace that arm. Nothing dispatches grep before then, so no unasked Cloud read is possible. `host.rs` untouched.
- `glob` dir param and `grep` dir param share semantics (relative to the root; must stay inside it); the glob walk itself is still M1-c and can reuse `search`'s walk.
- Review risk: `search::grep` walk is BFS per directory with `entries()`; the staged-view marker is not here (W-e3).
- `cargo test` showed one flaky failure under full-suite load (`shutdown_leaves_no_orphan_grandchild`), green alone and on rerun; unrelated.

## Checks (M1-b)

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (all green), `cargo +1.88 check --all-targets`. macOS-only files (`tests/jail_macos*.rs`, `runtime/jail*`) do not touch `Call`, `ToolEnv` or `tools::fs::Failure`.

## Batch 4: slice M1-c (search wiring and approval, PR 6, branch feat/coding-tools-m1c, base feat/coding-tools-m1b)

| Task | Status | Commit |
|------|--------|--------|
| M1c.0 spike: dispatch seam outside `host.rs` | done, `host.rs` NOT touched | (this docs commit) |
| M1c.1 `ApprovalAsk::Search`: asks on Cloud, free on Local, `Prompt` in every mode | done | feat(approval): ask before a cloud grep sends file text to the engine |
| M1c.2 execute glob/grep, prompt label, `Call::argument` with the folder | done | feat(tools): run glob and grep through the read-tool dispatch |
| M1c.3 end to end, Search off equals 0.18.0, Search on adds the tools | done | test(tools): run glob and grep end to end through the session, Search off and on |

## Spike result (M1c.0)

`host.rs` is NOT touched and there is NO new rebase point against agent-orchestrator S1c-2. `host.rs` already has a generic arm `(valid, locality) => ask = ask_for(valid, locality)` and executes through `tools::run` -> `fs::execute_with`. So the grep ask lives in `approval::ask_for` and the dispatch arms for `Call::Glob/Grep` live in `fs::execute_with`. The TurnScope/execute filter that S1c-2 changes are untouched.

## TDD cycle evidence (M1-c)

| Task | RED (seen failing) | GREEN | Triangulation | REFACTOR |
|------|--------------------|-------|---------------|----------|
| M1c.1 | `no variant named Search found for enum ApprovalAsk` (3 compile errors: approval.rs tests `grep_asks_on_cloud_with_pattern_and_path_and_runs_free_on_local`, `a_search_ask_prompts_in_every_mode`, and app test `a_search_ask_shows_the_pattern_and_the_path`) | `ApprovalAsk::Search`, `SEARCH_KEYS`, `ask_for` arm, prompt arm in `app/run.rs`, `app/tool.rs` arm | Cloud vs Local, dir None vs Some, sanitized one-line label, 3 modes x 2 origins, y/n in auto mode | none needed |
| M1c.2 | `cannot find function glob` (3 compile errors in `search::tests`); `call.argument()` test failed: left `("glob", "*.rs")` right `("glob", "*.rs in src")` | `search::glob`, shared `walk` extracted from `grep`, `fs::execute_with` arms, `argument()` with the folder | `**`/`*`/`src/*`, dir narrowing, escape, skip list at start (incl. mixed case), secrets marker, links, `../` pattern, budget, cancel, 101 cap, HOME unset; host: cloud sent/denied/unasked, local free, glob free on both | `walk` is shared by `grep` and `glob` (the 22 existing grep tests stayed green) |
| M1c.3 | with the dispatch arms temporarily reverted to `NotReadTool`, `with_search_on_glob_and_grep_run_end_to_end_and_a_cloud_grep_asks` failed: `{"result":{"content":[{"text":"not a read tool"...}],"isError":true}}` | (wired in M1c.2) | `with_search_off_glob_and_grep_are_neither_listed_nor_run` is a regression guard: green by design, the 0.18.0 golden (`golden.rs`) stays green | none needed |

The placeholder test `grep_is_not_executable_until_its_approval_is_wired` (fs.rs) is replaced by `grep_and_glob_are_dispatched_through_execute_with` (a Local grep, which has no ask, runs) plus host tests `a_cloud_grep_runs_only_after_the_answer_is_sent` and `a_cloud_grep_nobody_approves_is_denied_and_reads_nothing` (a Cloud grep asks first and opens nothing without `y`).

## Deviations / notes (M1-c)

- Ask label: `ApprovalAsk::Search { pattern, dir }`, both through `flat` (clean, one line, 512 chars); `dir` is `.` when the call names none (the spec says "root"). Prompt text: `search <pattern> in <dir>`.
- `Call::argument()` for glob/grep now returns `pattern in dir` when a folder is given (feeds the ToolCall summary); unchanged without one.
- glob matches the root-relative path of FILES only; `dir` only narrows the walk start. Output paths are root-relative. A bad glob pattern maps to `Failure::BadPattern` ("the pattern is not a valid expression"); the finer `GlobError` text is not carried.
- `grep`'s inline walk became `search::walk` (shared with `glob`); behavior of grep unchanged. `walk` loads the secret roots again after `execute_with` already did (one extra cheap load per call).
- Staged-view marker (`pending, not on disk`): NOT added, no consumer until W-e3 (`StagedView`); deferred there.
- `glob` stays unasked on every engine, as before.
- macOS-only files (`tests/jail_macos*.rs`, `runtime/jail*`) do not match on `ApprovalAsk`, `Call` or `Failure`.

## Checks (M1-c)

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (5429 lib tests green), `cargo +1.88 check --all-targets`.

## Batch 5: slice W-a (content rules and `resolve_write`, PR 7, branch feat/coding-tools-wa, base feat/coding-tools-m1c)

| Task | Status | Commit |
|------|--------|--------|
| Wa.1 content refusal table, existing-file check | done | feat(tools): refuse hostile content before it is staged for a write |
| (refactor) shared exact-label rule `paths::plain_relative` | done | refactor(tools): share the exact-label path rule between search and write |
| Wa.2 `resolve_write`, deny rules, tier-B `build_file` | done | feat(tools): resolve a write target only where the review can show it exactly |
| plan change: host.rs slices no longer blocked by S1c-2 | done | docs(openspec): lift the S1c-2 block on the host.rs slices of coding-tools |

## TDD cycle evidence (W-a)

| Task | RED (seen failing) | GREEN | Triangulation | REFACTOR |
|------|--------------------|-------|---------------|----------|
| Wa.1 | `error[E0425]: cannot find function check_existing in this scope` (29 compile errors in `content::tests`) | `ContentError`, `check_new`, `check_existing`, `is_hidden` (reuses `sanitize::is_format` + U+FFF9..FFFB, U+180E) | every C0 code, DEL + whole C1, CR alone, 29 listed invisible code points, neighbours that stay allowed, Latin-1/NUL/CR existing files, other controls allowed in an existing file. One test failed at runtime first: `a_refusal_names_its_kind_without_echoing_the_character` (message held a real newline from `\n` in the text; fixed to "LF line endings") | none needed |
| refactor | approval tests: the 22 `search::` tests stayed green before and after; 2 new `plain_relative` tests | `paths::plain_relative`, `shown_dir` = `plain_relative` + `root.join` | plain/dots, 11 label-changing inputs | the move itself |
| Wa.2 | `cannot find function resolve_write` / `cannot find type WriteTarget` (3 compile errors) | `WriteTarget{real, build_file}`, `resolve_write`, `is_build_file`, `WRITE_DENIED_DIRS`, `BUILD_FILES` | 11 tests: below-root resolution incl. missing folders, no creation, 11 refused roots, 10 escapes, 4 inside-links, 16 label-changing inputs, `.git`/`node_modules` at depth and case + look-alikes, 12 secret names, root inside `.ssh`, one `None` for every refusal, 18 build files vs 11 ordinary. One runtime RED: `ghp_0123456789abcdef...` was accepted (`clean` does not redact; `display_target` adds `redact` separately) -> `redact(input).hits > 0` refuses | none needed |

## Deviations / notes (W-a)

- The label rule lives once, in `paths::plain_relative` (extracted from `search::shown_dir`, behaviour unchanged). `resolve_write` is STRICTER than reads: the input must also equal its canonical text (`./a`, `a//b`, `a/`, `.` refused), any symlink anywhere on the way is refused even if it stays inside the root, and a secret-looking run in the path is refused (the read ask does it through `display_target`).
- `resolve_write(roots, origin, input) -> Option<WriteTarget>`: `None` is the single refusal (`REFUSED` text). `WriteTarget.real` is the origin's real path joined to the input; `build_file` is the tier-B flag (the warning text belongs to the review, W-e1).
- Root rules: absolute, canonical, a directory, strictly inside HOME (`SecretRoots` already refuses HOME = system folder), not itself under a secret root. A `/usr`, `/home`, `/`, parent-of-HOME or outside-HOME origin all refuse through "strictly inside HOME".
- Content: `is_hidden` = `sanitize::is_format` (a superset of the design list: it also covers U+2028/2029 and U+206A..206F) + U+180E + U+FFF9..FFFB. Refused kinds: `Control`, `CarriageReturn`, `Invisible`, `NotText`. Messages never echo the character.
- An existing file may hold other controls (the review shows `<U+XXXX>`, W-e1); only non-UTF-8, NUL and CR block an edit/overwrite.
- `check_new` takes `&str`, so "invalid UTF-8 in written content" cannot occur there; it is covered by `check_existing`.
- Not here (later slices): size caps (Wc.2), the unredacted-overwrite rule (Wc.3), the `.git/config` read deny (already in M1-b).
- The docs commit lifts the BLOCKED status of W-f..h, X-d1/2, G-c/d in tasks.md and keeps the "spike a seam first" rule. `design.md` still says (lines 155 and 175) that these slices wait for S1c-2; it was not edited.
- macOS-only files (`src/runtime/jail*`, `tests/jail_macos*.rs`, `core/ai/jail.rs`) do not touch the new items.

## Size (W-a)

About 173 production lines (content.rs 71, paths.rs 108, search.rs -7, mod.rs 1) and about 400 test lines; `git diff --stat feat/coding-tools-m1c...HEAD -- src`: +575/-18.

## Checks (W-a)

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (5455 lib tests green), `cargo +1.88 check --all-targets`.

## Batch 6: slice W-b (write primitives, PR 8, branch feat/coding-tools-wb, base feat/coding-tools-wa)

| Task | Status | Commit |
|------|--------|--------|
| Wb.1 `open_target` (parent beneath HOME, final link, nlink, non-regular, `judge_real` on the fd path) | done | feat(tools): open a write target beneath HOME and refuse links, hard links and non-files |
| Wb.2 `make_dir`, `create_file`, `replace_file` | done | feat(tools): write files and folders through a same-folder temp and an atomic rename |
| W-a review obligations (6) + design D22/D23, W-d decision | done | docs(openspec): record the write primitive design and the check_new scope |

## TDD cycle evidence (W-b)

| Task | RED (seen failing) | GREEN | Triangulation | REFACTOR |
|------|--------------------|-------|---------------|----------|
| Wb.1 + Wb.2 (tests written together, 41 compile errors) | `error[E0425]: cannot find function open_target / make_dir / create_file / replace_file / temp_name in this scope` and `error[E0433]: cannot find type WriteError in this scope` (41 errors in `write::linux::write_tests`; the tests were first written in `open.rs`) | `WriteError`, `open_parent`, `open_in`, `open_target`, `make_dir`, `create_file`, `replace_file`, `Temp` drop guard, `temp_name` (getrandom), `stage`, `rename`, `sync_dir`; 14 write tests green at once | present file / absent; final link inside and dangling; folder swapped for a link after the check; hard link ok vs nlink 2; folder and FIFO; `.ssh` and outside HOME; make_dir new / exists / missing parent / symlinked parent; create bytes with BOM / empty / name taken (temp cleaned) / link in the way / dangling link target / missing parent; replace at two modes (0o750, 0o640) / hard link / link / absent; temp names differ | the rename guard first used an unsafe pointer cast; changed to `&mut Temp` before the first commit |

Note: the RED was a compile failure only (the functions did not exist), then every test passed on the first GREEN run; no test needed a fix. The slice was committed as two work units by splitting the finished file (Wb.1 subset first, then the full file). The full-suite run then showed the audit failure, which led to the move to `write.rs` (the first cut had the code in `open.rs`).

## Deviations / notes (W-b)

- DEVIATION: the write primitives live in the new `runtime/tools/write.rs`, not `open.rs`. `audit.rs` (`the_tool_code_starts_no_process_opens_no_socket_and_creates_no_file`) bans `O_CREAT`, `O_WRONLY`, `write_all`, `set_permissions`, `libc::syscall` other than `SYS_openat2` and others from `fs.rs`, `open.rs` and `mod.rs`; it failed when the code was in `open.rs`. `open.rs` only widens four helpers (`flags`, `cstring`, `owned`, `open_raw`) to `pub(in crate::runtime::tools)`. A new audit test, `the_read_side_never_names_the_write_primitives`, keeps `fs.rs`, `open.rs` and `search.rs` from naming `tools::write`. The module reuses `open_checked` (openat2 + `fd_path == real` + `judge_real`) for the parent. The syscalls use `libc` like the read side (no new dependency); `renameat2` goes through `libc::syscall(SYS_renameat2)`, `getrandom` through `libc::getrandom`.
- API (all take `&SecretRoots` and the already resolved real path): `open_target -> Result<Option<File>, WriteError>`, `make_dir`, `create_file(bytes)`, `replace_file(bytes)`. `WriteError = Refused | Exists | Missing | Unsupported | Io`. A missing parent folder is `Refused` (open_checked does not tell ENOENT apart).
- `replace_file` restores the mode (`fchmod` to `mode & 0o7777`); owner and group are not restored. It renames over the name, so a late hard link or link is replaced, not written through. There is a small window between the open check and the rename where the name can change; the rename never writes through it, and the apply preflight (We2) compares bytes.
- The parent folder is synced best effort (errors ignored): some filesystems refuse it.
- A filesystem without `RENAME_NOREPLACE` makes `create_file` fail with `Io`; it never falls back to a replace.
- `check_new` scope: decided in design D23 (new text only); the test belongs to Wd.1 (recorded in tasks.md). BOM asymmetry documented there.
- Non-Linux: `write.rs` has the same signatures returning `Unsupported`; one test per platform arm.
- macOS: `cargo check` and `clippy --all-targets --target aarch64-apple-darwin -D warnings` pass. The sqlite C build needs a macOS compiler, so it was run with `CC_aarch64_apple_darwin=true AR_aarch64_apple_darwin=true` (stub C compiler, check only).
- No change to `host.rs`; no wiring.

## Size (W-b)

About 290 production lines and about 260 test lines (`write.rs` new, `open.rs` +4/-4, `audit.rs` test, `mod.rs` +1; see `git diff --stat feat/coding-tools-wa...HEAD -- src`), plus docs.

## Checks (W-b)

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo +1.88 check --all-targets`, aarch64-apple-darwin check and clippy.

## Batch 7: slice W-c (stage state machine, PR 9, branch feat/coding-tools-wc, base feat/coding-tools-wb)

| Task | Status | Commit |
|------|--------|--------|
| Wc.1 `Batch` lifecycle, `StageKey::Pane`, `BatchId`, orphans | done | feat(tools): add the write batch state machine with orphans and stale answers |
| Wc.2 caps at stage and propose, `write_file` 32 KiB | done | feat(tools): cap a write batch at 50 entries and 1 MiB when staging and when proposing |
| Wc.3 overwrite only when `<= 64 KiB` and `redact(clean(text)) == text` | done | feat(tools): overwrite an existing file only when its text is what the review shows |

## TDD cycle evidence (W-c)

| Task | RED (seen failing) | GREEN | Triangulation | REFACTOR |
|------|--------------------|-------|---------------|----------|
| Wc.1 | `error[E0422]: cannot find struct ... BatchId`, `error[E0425]: cannot find type Batch / StageKey`, `cannot find type Entry / Orphan` (43 compile errors) | 12 tests green at once | create/restage/dir-as-file replace, empty propose, staging refused while Proposed, both answers close the batch, stale generation and foreign key, resolve before propose, discard from both states, orphan for both states and live turn | none needed. Mutation: `if false && id != self.id` made `a_stale_generation_is_refused_and_changes_nothing` fail |
| Wc.2 | `error[E0425]: cannot find value MAX_ENTRIES / MAX_BYTES / WRITE_FILE_MAX`, `no method named bytes` (27 compile errors) | 19 green after fixing one test arithmetic mistake (my restage test overfilled the cap by 5 bytes) | 32 KiB at and over, entry 50 ok and 51 refused, restage at 50 ok, 1 MiB boundary, restage counts the replacement, propose recheck of both caps leaves the batch unchanged, exact caps propose ok. Mutation: propose recheck replaced by `if false` made `propose_rechecks_the_caps_and_leaves_the_batch_unchanged` fail | none |
| Wc.3 | `cannot find type Existing`, `READ_CAP`, `StageError::Content`, `variant Entry::File has no field base` (79 compile errors, the 30 old `stage_file` calls too) | 26 green at once | clean 2 KiB+ overwrite keeps its base, API key refused, exactly 64 KiB ok / +1 refused / huge judged by size, ESC/bidi/BEL on disk refused, CRLF and non-UTF-8/NUL keep their content refusal, `check_new` on the new text for create and overwrite, refused overwrite keeps the earlier entry. Mutations: `size >= READ_CAP` and `clean(disk) != disk` (no redact) each made tests fail | none |

## Deviations / notes (W-c)

- `gen` is a reserved keyword in edition 2024, so `BatchId.generation` replaces the design's `gen`.
- `stage.rs` is pure. `Batch` is one batch; the per-key store (`Stages`, one batch per key, generation counter) is We3.1. Nothing is wired, `host.rs` untouched.
- API: `Batch::{new, id, state, entries, bytes, stage_dir, stage_file(target, text, Existing), propose, resolve(id, Outcome), discard, orphan(turn_live)}`. The caller reads the disk: `Existing::Absent | Present{size, bytes}` (bytes needed only when `size <= READ_CAP`). `Entry::File{text, base}` keeps the disk text of an overwrite for the diff and the rollback.
- `stage_file` also runs `content::check_new` on the new text (design D23); a create and an overwrite both get it.
- Caps: stage time counts the replacement (a restage of a staged path does not double count); `Full` = "batch is full: propose it, then stage the rest"; propose re-check = `TooLarge` ("batch is over 50 entries or 1 MiB: split it into smaller batches"). `write_file` over 32 KiB = `FileTooLarge`. Dirs count as entries, 0 bytes.
- Orphan: `orphan(turn_live)` is `None` for an empty batch, a live turn, or a closed batch; otherwise `Staging` (discard only) or `Proposed` (review and apply).
- Not here: the 1 MiB file cap and per-field caps of `edit_file` (W-d), the pending-view marker (We3.2), apply (We2).

## Size (W-c)

About 304 production lines (`stage.rs` lines before `mod tests`, with comments) plus 1 in `tools/mod.rs`; about 467 test lines. `git diff --stat feat/coding-tools-wb...HEAD`: 2 source files, +772 (before docs).

## Checks (W-c)

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (5516 lib tests green), `cargo +1.88 check --all-targets`.
