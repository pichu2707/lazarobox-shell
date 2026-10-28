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

## Batch 8: slice W-d (edit and diff, PR 10, branch feat/coding-tools-wd, base feat/coding-tools-wc)

| Task | Status | Commit |
|------|--------|--------|
| Wd.1 `edit_file` on `Batch` (`tools/edit.rs`) | done | feat(tools): stage an edit of one exact text against the staged or disk version of a file |
| Wd.2 line diff (`tools/diff.rs`, `similar`) | done | feat(tools): diff a staged file against its disk text with a deadline and a coarse fallback |

## TDD cycle evidence (W-d)

| Task | RED (seen failing) | GREEN | Triangulation | REFACTOR |
|------|--------------------|-------|---------------|----------|
| Wd.1 | `error[E0599]: no method named edit_file found for struct Batch` (30), `E0433 cannot find type EditError` (19), `E0425 cannot find value FIELD_MAX / FILE_MAX / function apply_edit` (56 compile errors) | 17 tests; one failed first because of a test bug (a second call on a path already staged), fixed in the test | unique / none / many / overlapping (`aa` in `aaa`), empty old, chained edit keeps the disk base and ignores the disk text passed later, edit of a staged create (base None), missing file, staged folder, field cap 16 KiB at and over, file cap 1 MiB at and over and by size alone, short read, Latin-1 and CRLF disk, BOM and ESC on disk editable elsewhere, `new` with BOM / BEL / CR refused, Proposed state refused, batch cap leaves the batch unchanged. Mutations: removing `check_new(new)` failed `a_new_text_with_a_bom_or_a_control_is_refused`; a non-overlapping second-match search failed `overlapping_matches_count_as_many` | none |
| Wd.2 | `error[E0425]: cannot find function diff / diff_within`, `cannot find type Diff / Kind` (20 compile errors) | 11 tests; two failed first because of test expectations (hunk start is the first context row; a context row before a changed last line), fixed in the tests | create all added, empty create, 3-line context, two hunks, identical texts, missing newline on removed / added / context rows and as `\ No newline at end of file`, coarse on deadline, coarse create equals the normal one, determinism, 20k lines inside the deadline. Mutations: no deadline fallback failed the coarse test; `missing_newline` forced false failed 2; context 1 instead of 3 failed the context test | none |

## Deviations / notes (W-d)

- `similar 2.7.0` was already in `Cargo.lock` through `insta` (dev-dependency), so no new crate is downloaded; it is now a normal dependency (`similar = "2"`, default features). MSRV check with `cargo +1.88 check --all-targets` passes. No other diff crate is a dependency.
- API: `Batch::edit_file(target, old, new, existing) -> Result<(), EditError>` is an `impl Batch` block in `edit.rs`; `apply_edit(current, old, new)` is the pure replace. `stage.rs` only widens `put` and `open` to `pub(super)` and adds `staged(&self, target)`.
- `Entry::File.base` invariant kept: an edit of an unstaged existing file stores the disk text as `base`; an edit of a staged path keeps the staged entry's `base` (so a create stays `None`, and a chained edit keeps the first disk text). The cap logic is not duplicated: `put` still enforces the entry and byte caps, and a failed `put` leaves the batch unchanged.
- `old` uniqueness counts overlapping occurrences (`aa` in `aaa` is ambiguous).
- Staged text wins: when the path is staged, the `existing` passed in is ignored (not even read for size). The caller still reads disk for an unstaged path; W-e2's drift check compares that `base` with the disk at apply time.
- Spec/design give no error texts for `edit_file` beyond "refused with an error result"; the texts are new: `old must not be empty`, `old does not occur in the file`, `old occurs more than once: add context so that it occurs exactly once`, `old and new must each be at most 16 KiB`, `the file is over 1 MiB and cannot be edited`, `the file does not exist: use write_file to create it`.
- Deadline decision (spec and design say only "Myers, 500 ms deadline"): `similar` gets the deadline, and if the run took `>= 500 ms` the minimal result is discarded for a coarse diff (all base lines removed, all new lines added, one hunk, `Diff::coarse = true`). Chosen over keeping `similar`'s own degraded output because that output is not specified and cannot be detected. A create never goes coarse. Purity: `diff` is a function of (base, new) except that the choice between minimal and coarse depends on machine speed; both are correct diffs. `diff_within(base, new, limit)` makes this testable (`Duration::ZERO`).
- Hunk starts are 1-based; an empty side starts at the line before (unified-diff convention). `Display for Diff` renders the unified text with the `\ No newline at end of file` marker; We1's `ReviewDoc` can use the structured rows instead.
- Redaction: `edit_file` does not require `redact(clean(base)) == base` (only `write_file` overwrite does, per spec); a secret in `base` would show in the review diff to the user, not to the model.
- Coverage gate is W-e1. Files are I/O-free and live under `core/ai/tools/` (not audited). No change to `host.rs`; no wiring; no macOS-only file touched.

## Size (W-d)

About 305 production lines (edit.rs 120, diff.rs 175, stage.rs +10, mod.rs +2, Cargo.toml +3) and about 390 test lines; `git diff --stat feat/coding-tools-wc...HEAD`: +698/-2 before docs.

## Checks (W-d)

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (5553 lib tests green), `cargo +1.88 check --all-targets`.

## Batch 9: slice W-e1 (ReviewDoc and coverage core, PR 11, branch feat/coding-tools-we1, base feat/coding-tools-wd)

| Task | Status | Commit |
|------|--------|--------|
| We1.1 `ReviewDoc` (`core/ai/chat/review.rs`) + review-only obligation | done | feat(chat): build the review document of a staged batch with escaped rows and warnings |
| We1.2 `layout`, `Coverage`, 800 ms gate | done | feat(chat): lay out the review once and gate approval on full coverage and an 800 ms delay |

## TDD cycle evidence (W-e1)

| Task | RED (seen failing) | GREEN | Triangulation | REFACTOR |
|------|--------------------|-------|---------------|----------|
| We1.1 | `error[E0425]: cannot find type LineKind / ReviewDoc in this scope`, `E0433` on both (31 compile errors) | 25 of 26 at once; one failed because my test staged an invisible char in NEW text, which `check_new` refuses (test fixed to use `edit_file` over a disk text with U+202E / ESC) | create, overwrite (context/removed/added), dir, escapes in context and removed rows, build-file warning vs plain file, coarse (via `diff_within(.., ZERO)`), truncated (2100 rows, totals exact), no-newline note, tab expansion. Review-only: autoref probe (self-tested) shows no `Display`, `ToString`, `Into<String>` on `ReviewDoc`/`DocLine`; `Debug` prints counts and never a row. Mutations: `display_text` -> `raw_text` failed the escape test; build warning off failed the warning test; a temporary `impl Display for ReviewDoc` failed `a_review_has_no_path_into_a_string` | none needed |
| We1.2 | `error[E0425]: cannot find function layout / type Layout / value ARM_DELAY`, `E0433 cannot find type Coverage` (23 compile errors) | 41 of 41 after fixing one test (an invisible char in new text again) | wrap at width, rejoin of rows equals the line, CJK width 2, `<U+200B>` counted as 8 columns, width 0 progresses, line seen only when ALL rows shown (accumulates across calls), clamped range, resize keeps seen lines, half-seen rows dropped on resize, gate (unseen blocks, 799 ms no, 800 ms yes, clock going back no). Mutations: `>= ARM_DELAY` -> `>` failed the gate test; coverage check removed failed it; `all` -> `any` failed 2; removing the width check passed at first (the row-count check hid it), so a test with two widths of equal row count (62 x's at 30 and 31) was added and then failed under the mutation | none needed |

## Deviations / notes (W-e1)

- API: `ReviewDoc::{build(&Batch), build_with(&Batch, diff_fn), lines, files, added, removed}`; `DocLine{kind: LineKind, text}` where `LineKind = Summary | File | Warning | Hunk | Context | Added | Removed | Note`; the text of a diff row is the sign (` `, `+`, `-`) plus `Line::display_text()`, with tabs expanded to spaces (`wrap::expand_tabs`) so columns are measurable. `layout(&ReviewDoc, width) -> Layout{rows() -> [Row{line, text}]}`; `Coverage::{new(&doc, opened: Instant), show(&layout, rows: Range<usize>), total, remaining, can_approve(now: Instant)}`; `ARM_DELAY = 800 ms`.
- Clock: the core takes `opened` and `now` as `Instant` parameters, so it has no clock of its own; We4 passes the app's injected `(self.clock)()`. A test needs no sleep. `now` before `opened` does not arm (`saturating_duration_since`).
- Coverage unit is the LOGICAL line (summary, file header, warning, hunk header, each row, note), so the gate counter reads "N lines remaining". Seen lines survive a resize; rows shown at another width are dropped, so a line half shown at one width is not seen at another (a stricter reading of "all its wrapped rows were shown").
- Wrapping reuses `chat/wrap.rs::wrap_ranges` (grapheme clusters, `unicode-width`), a hard wrap by display width. A `<U+XXXX>` escape can break across two rows; each row stays within the width.
- Only change outside the new file: `diff.rs` gets a public `escape(&str)` (the body of `Line::display_text`, now shared) so paths are escaped with the same rule. No behaviour change there.
- Warnings: build file (`WriteTarget::build_file`), coarse diff, truncated diff. The `no newline at end of file` is a `Note` row, not a warning. Warning texts are new: `warning: build file: it can run code when the project is built`, `warning: coarse diff: the whole file is shown as replaced`, `warning: truncated: rows were left out; the totals are exact`.
- Review-only obligation: `ReviewDoc` and `DocLine` have no `Display`, `Serialize`, `Into<String>`; `Debug` is hand-written (counts only); `DocLine` has no `Debug`. `Diff` itself keeps its `Display` (W-d) and is never stored in the doc. The model-facing answer (`denied` / apply result / `propose_batch` result) belongs to W-f and W-e4; the obligation was recorded in W-e4 in tasks.md next to the existing one in W-f. Nothing in this slice builds a model-facing summary.
- Not here: rendering (We4), apply (We2), `Stages` (We3). host.rs untouched, nothing wired; no macOS-only file touched (the shared `diff.rs` change is a refactor).

## Size (W-e1)

About 300 production lines (`review.rs` before `mod tests`: 287, `diff.rs` about +17, `mod.rs` +1) and about 410 test lines. `git diff --stat feat/coding-tools-wd...HEAD`: 3 files, +712/-11 before docs.

## Checks (W-e1)

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (5591 lib tests green), `cargo +1.88 check --all-targets`.

## Batch: slice W-e2 (apply, PR 12, branch feat/coding-tools-we2, base feat/coding-tools-we1)

| Task | Status | Commit |
|------|--------|--------|
| We2.1 `WriteOps`, `Disk`, preflight, `Drifted` | done | feat(tools): preflight every target of a reviewed batch and refuse the whole batch on drift |
| We2.2 journal, ordered apply, reverse rollback, `delete_file`/`delete_dir` | done | feat(tools): journal an apply and undo it in reverse when a step fails |
| We2.3 `Partial{changed}`, `Change` records, `summary()` | done | feat(tools): report a failed undo as partial and return a change record per applied path |
| Obligations from the W-a review | done | docs commit (tasks + progress) |

## TDD cycle evidence (W-e2)

| Task | RED (seen failing) | GREEN | Triangulation | REFACTOR |
|------|--------------------|-------|---------------|----------|
| We2.1 | `E0405 cannot find trait WriteOps`, `E0425 cannot find function apply`, `E0433 cannot find type ApplyResult/WriteError` (30 compile errors). Audit: with `apply.rs` present but unlisted, `every_tool_file_...` failed with `apply.rs: a reference to the write module` | 7 of 7 | overwrite drift (3 entries, only the middle one drifts, nothing written), create onto a taken name (all drifted paths listed), vanished file, existing folder and refused target, clean batch, target below a batch folder (no probe), probe Io -> Failed / Unsupported. Real disk: clean batch, symlink target (nothing written through), hard link and changed file. Mutations: `bytes == base` -> `true` failed 3 tests. The real-disk symlink test passed at once and is NOT protected by a mutation of `open_target` -> `unwrap_or(None)` (a base mismatch also drifts); the symlink refusal itself is pinned in `write.rs` tests (Wb.1) | none needed |
| We2.2 | write.rs: `E0425 cannot find function delete_file / delete_dir` (9 errors); apply: `E0407 method delete_dir/delete_file is not a member of trait WriteOps` | 14 of 14 after fixing my own test (fault index 3, not 4) | fault at step 3 of 3 (edit restored, create removed, undo order asserted), nested folders (shallowest first, undone deepest first), fault at step 0, race drift after the preflight -> `Drifted` after the undo, real disk with a read-only folder. write.rs: delete only while content equals, link, hard link, folder, denied path, empty vs full vs link folder. Mutations: undo iterated forward failed 3; undo removed failed 3 | `holds()` shared by `replace_file` and `delete_file` |
| We2.3 | `E0599 no variant named Partial`, `no method named summary` (8 errors) | 20 of 20 | failing undo of the first file; folder left because its file stayed; undo finds a swapped file (`theirs`) and leaves it; `Change` for create/edit/folder with line counts; `summary()` for every variant with a secret never printed (also `Debug`). Mutations: `Partial` arm disabled failed 3; `Edited`/`Created` swapped failed 2; `removed` -> `added` failed 2 | none needed |

## Deviations / notes (W-e2)

- API (`runtime/tools/apply.rs`): `trait WriteOps {probe, make_dir, create_file, replace_file, delete_file, delete_dir}`, `Disk::new(&SecretRoots)`, `apply(&dyn WriteOps, &[Staged]) -> ApplyResult`, `ApplyResult::{Applied(Vec<Change>), Drifted(Vec<PathBuf>), Failed{path}, Partial{changed}, Unsupported}`, `ApplyResult::summary()`. W-e3 calls `apply(&Disk::new(roots), batch.entries())`.
- Design deviations: `Failed{path}` instead of `Failed{rolled_back}` (a failed rollback is `Partial`, so the flag would always be true; the path says where it failed); added `Unsupported` (no platform support / `openat2` missing); `ChangeKind` is `Created | Edited | DirCreated` (a staged entry cannot tell `write_file`-over-existing from `edit_file`, so both are `Edited`, no `Overwritten`).
- The journal is the list of finished entries; the undo is derived from the entry (folder -> `delete_dir`; create -> `delete_file(.., text)`; overwrite -> `replace_file(.., expected = text, bytes = base)`). The undo keeps going past a failure and `Partial.changed` lists the failed undos in the order met (reverse apply order). A folder whose file could not be removed is listed as well (it is not empty).
- Preflight reads each target through `open_target` (the checked parent, `O_NOFOLLOW`, nlink 1, `judge_real` on the fd path). A target under a folder created by the same batch is not probed (absent by construction). A folder target must still be absent. `Refused`/`Drifted` from a probe = drift (lists every drifted path); `Io` = `Failed`; `Unsupported` = `Unsupported`. A race after the preflight (a step answers `Drifted`) rolls back and reports `Drifted([path])`.
- Names: write.rs gets `delete_file(roots, real, expected)` and `delete_dir(roots, real)`, not `remove_*`: `remove_file`/`remove_dir` are `FS_WRITE` tokens and `apply.rs` must not contain one, not even as a trait method name. `delete_file` refuses (`Drifted`) when the content is no longer what we wrote, so an undo never deletes a file someone replaced (it is then reported in `Partial`). `delete_dir` is `unlinkat(AT_REMOVEDIR)`: never follows a link, fails on a non-empty folder. Off Linux both return `Unsupported` (darwin `cargo check --all-targets` green).
- Review-only invariant: `ApplyResult`/`Change` hold paths and counts only; `summary()` is the only text for the model and has a test with a secret in the content.
- Obligations from the W-a review: met by the write.rs primitives (re-open along the chain, nlink, final symlink, judge_real on the fd path); checked by real-disk tests in `apply.rs`. D23 (check_new on new text only, BOM asymmetry) needs nothing here: apply writes the staged bytes verbatim.
- Not here: `Stages` store / `BatchId` wiring (We3), the review view and the answer wiring (We4), host tools (Wf). `host.rs` untouched.

### Audit changes (exactly two)

1. `MAY_CALL_WRITE` gets `"apply.rs"` (doc comment updated). New test `apply_may_name_the_write_module_but_never_a_write_api_itself`: the real `apply.rs` references `write` (so the entry is needed) and has no problem; planted write tokens in a file called `apply.rs` are still flagged; `host/apply.rs` earns nothing.
2. `WRITE_LIBC_ALLOWED` gets `"AT_REMOVEDIR"` (`unlinkat` was already there).

## Size (W-e2)

About 380 production lines (`apply.rs` before tests 312, `write.rs` about +63, `audit.rs` +4 non-test, `mod.rs` +1) and about 700 test lines; `git diff --stat feat/coding-tools-we1...HEAD` before docs: 4 files, +1083/-10.

## Checks (W-e2)

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (5624 lib tests green), `cargo +1.88 check --all-targets`, `cargo check --all-targets --target aarch64-apple-darwin`.

## Batch: slice W-e3 (Stages store and StagedView, PR 13, branch feat/coding-tools-we3, base feat/coding-tools-we2)

| Task | Status | Commit |
|------|--------|--------|
| We3.1 runtime `Stages` store (seal/apply/discard by `BatchId`, never-reused generation) | done | feat(tools): keep staged batches in a runtime store with generations that are never reused |
| We3.2 `StagedView` overlay for `read_file`, `glob`, `grep`, `list_dir` | done | feat(tools): let the read tools see staged changes, marked as pending and under the same rules |
| Obligation from the Wc review (generation) | done | in We3.1 |

## TDD cycle evidence (W-e3)

| Task | RED (seen failing) | GREEN | Triangulation | REFACTOR |
|------|--------------------|-------|---------------|----------|
| We3.1 | `E0425 cannot find type Stages/StageKey/BatchId`, `E0405 cannot find trait WriteOps`, `E0433 StageError/ApplyResult` (29 errors). Audit RED: with `stage.rs` present and unlisted, `every_tool_file_...` failed with 3x `stage.rs: a reference to the apply module` | 11 of 11 | one batch per key, two keys, seal (empty, then staging refused), apply once then stale, apply before seal, drifted apply stays proposed then discard, discard staging and proposed, old id refused by the new batch of the same key, generations grow across keys, counter at `u64::MAX` refuses, pane close. Mutation: counter not advanced (`checked_add(0)`) failed 3 tests (obligation, overflow, growth). The new audit test `stage_may_name_the_apply_module...` passed at once after the list entry; its body is the same `file_problems` that failed in the tree test | `Entry` API for get-or-create instead of contains+get |
| We3.2 | `E0433/E0425 cannot find type StagedView` (8 errors), `unresolved import fs::execute_over` | 11 of 11 after fixing two of my own expectations (`id_rsa` is not an excluded name, use `.npmrc`; staged-only files come after the disk files in grep) | read of edit and create (disk untouched), window of staged text, redaction of staged text (read and grep), staged `.env`/`.git/config`/`.npmrc`/temp name/`target/` refused or hidden, glob+grep create and edit with marker, `dir` narrows, list_dir (existing folder, folder not on disk, staged folder, unknown folder refused), forged marker in content and in a name, empty view equals `None` equals plain execute for all 4 tools, snapshot. `Stages::view` test (RED: `no method named view`). Mutations: no `judge_real` in `locate` failed 1; no secret-name check in `staged_only` passed until I added the `sk-...` name test, then failed 1; no defang failed 1; no redaction in `prepared` failed 1; skipped-dir check removed failed 1; window ignored failed 1 | `head_text`/`window_text` extracted from `read_head`/`read_window` (bytes in, text out); `prepared` shared by `read_text` |

## Deviations / notes (W-e3)

- API: `Stages::{stage(key) -> Result<&mut Batch, Exhausted>, batch(key), view(key) -> StagedView, seal(key), apply(id, &SecretRoots), discard(id), discard_key(key) -> bool}`. `apply` is `apply_with(id, &Disk::new(roots))`; `apply_with(&dyn WriteOps)` stays private for the tests. One generation counter for ALL keys (`u64`, `checked_add`, starts at 0): it only grows, so a generation is never reused across keys either. `Exhausted` is its own type, so the pure `StageError` is untouched.
- Closed batches leave the store: `Applied` and `Partial` (the batch can no longer be applied as reviewed) are removed; `Drifted`, `Invalid`, `Failed` and `Unsupported` leave it `Proposed` (nothing is on disk), so the user can still discard it. A repeated answer for a removed batch is `Stale`.
- `discard_key` (pane close and quit) returns whether a batch WITH entries was dropped, so W-g knows whether to show the notice. Orphan review (`review_orphan`) is not wrapped here: W-g calls `batch.review_orphan` through `stage()`/`batch()` when it needs it.
- `StagedView` lives in `runtime/tools/staged.rs`: owned `files: BTreeMap<PathBuf, String>` and `dirs`, built by `from_batch(&Batch)` (empty for a closed batch). It is a snapshot: later staging does not change it.
- Seam (no `host.rs`, no `ToolEnv` change, whose 12 literals would all break): `fs::execute_over(call, env, limits, cancel, Option<&StagedView>)`; `execute_with` is now `execute_over(.., None)`. `search::{grep_over, glob_over}` the same, with `grep`/`glob` as thin wrappers. W-f passes the view of the key. `mod.rs::run` is unchanged.
- Marker text (spec): `(pending, not on disk)`. read_file: first line, then the text. glob: `path (pending, not on disk)`. grep: `path (pending, not on disk):LINE:text`. list_dir: `f name (pending, not on disk)`, only for names NOT on disk (an edited file keeps its plain row; its content is the staged part and is marked in `read_file`/`grep`/`glob`).
- Forgery: while a view is in use (even an empty one), the words `pending, not on disk` in disk text, file names and `find_dir` output become `pending; not on disk`. Without a view nothing changes (byte-identical to before; a test compares all four tools). W-f must pass a view whenever the write family is on, so the marker is unforgeable.
- Same protections: a staged path is judged by the call's roots (`judge_real`: HOME, secret names and roots, `.lbx-*.tmp`), `is_git_config` refuses it, and for glob/grep also the skip-list folders, `redaction_changes` on names and path (left out and noted with the omitted marker). Staged text goes through `clean` + `redact`, with the same read window and cap as a file (`head_text`/`window_text`), and the 1 MiB search cap.
- A staged-only file is visited after the disk walk (so grep order is disk files first, then staged-only ones; glob sorts anyway). A `dir` argument and the depth bound apply to them.
- Known limits: the staged lookup is by lexical path (`display_target`), valid because `resolve_write` stages only canonical paths with no link on the way; the apply preflight is what detects later drift. `list_dir` on a staged-only folder lists only staged names.
- Not here: host wiring and passing the view (W-f), orphan effects and quit notice (W-g, W-h). `host.rs` untouched. No macOS-only file touched (`jail*`, `tests/jail_macos*`).

### Audit changes (exactly one)

`MAY_CALL_APPLY` gets `"stage.rs"` (doc comment updated). New test `stage_may_name_the_apply_module_but_holds_no_write_api_and_not_the_write_module`: the real `stage.rs` references `apply` and has no problem; every write token and a `write` module reference planted in a file called `stage.rs` is still flagged; `host/stage.rs` earns nothing. `staged.rs` needs no entry.

## Size (W-e3)

About 435 production lines inserted (`stage.rs` 114 before tests, `staged.rs` 135, `fs.rs` about +70, `search.rs` about +110, `audit.rs` +4 non-test, `mod.rs` +2) and about 640 test lines. Over the ~350 target, under the 500 stop. `git diff --stat feat/coding-tools-we2...HEAD` before docs: 6 files, +1054/-30.

## Checks (W-e3)

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo +1.88 check --all-targets` green. `cargo check --target aarch64-apple-darwin` could not run here (the C compiler for sqlite fails for arm64 on this host); no macOS-only file uses the changed items.

## W-e3 review fixes

- Lookup: `StagedView::locate` resolves with the new `paths::real_target` (`canonical_new` of the origin-joined path, no `..` folded by text) after the same `display_target` text gate. Fixes a symlinked origin, a read through a symlinked folder, and `link/../x`. RED: 4 assertion failures (symlinked origin, symlinked folder, `lnk/../x.txt` showed staged root text instead of disk `src/x.txt`).
- `Stages::seal(id: BatchId)` is generation-checked via `current(id)` (RED: `E0308 mismatched types` on `seal(old)`/`seal(new)`).
- `Stages::stage_with(key, f) -> Result<T, StageFailed>` removes the batch again when this very call made it and `f` failed; an existing batch stays (RED: `E0599 no method stage_with`, `E0433 StageFailed`).
- `plain`/`plain_str` no longer take the view: defang is unconditional (see design.md). RED: `left: "fn (pending; not on disk)" right: "fn (pending, not on disk)"` for None vs Some(empty). The tautological `execute` vs `None` comparison is dropped.
- `list_dir` sets `omitted` when a staged-only name is hidden (excluded, outside the judge, or secret-like). RED: listing ended `f notes.txt\nd src` with no omitted marker.
- `search.rs`: `shown` loses its unused view parameter.

## Batch: slice W-e4 (review view, PR 14, branch feat/coding-tools-we4, base feat/coding-tools-we3)

| Task | Status | Commit |
|------|--------|--------|
| We4.1 `ApprovalAsk::Batch`, `BatchReview`, `ReviewView` state, `app/review.rs` keys and gate | done | feat(chat): ask the user to review a write batch, with y gated by coverage and n or Esc rejecting it |
| We4.2 full-pane view `ui/components/review.rs`, warnings, seen counter, countdown | done | feat(ui): draw the batch review over the chat pane with warnings, a seen counter and the arm countdown |
| We4.3 two named delays (`ASK_ARM_DELAY` 400 ms, `review::ARM_DELAY` 800 ms) | done | refactor(chat): name the two approval delays, 400 ms for every ask and 800 ms for a batch review |
| Unreviewable batch decision | done | in We4.1 (answer `TOO_LARGE`, `y` never arms) and We4.2 (view offers `n discard` only) |
| Wd-review obligation (review-only invariant) | done | test `a_secret_in_the_base_or_the_new_text_reaches_nothing_but_the_renderer` in We4.1 |

## TDD cycle evidence (W-e4)

| Task | RED (seen failing) | GREEN | Triangulation | REFACTOR |
|------|--------------------|-------|---------------|----------|
| We4.1 | `ReviewView` tests: `E0432 unresolved import BatchReview`, `E0433 cannot find type Move/ReviewView` (23 errors). Batch ask: `E0599 no variant Batch for ApprovalAsk`, `no method review/set_review for Approvals` (12 errors). App: `E0004 non-exhaustive patterns ApprovalAsk::Batch(_) not covered` in `run.rs` and `tool.rs`. Behavioural RED in the view: `left: 13 right: 0` (jumping to the end does NOT see the skipped rows, so my first expectation was wrong and the gate right; tests now page through) | 12 of 12 view, 4 of 4 core, 11 of 11 app | jump vs paging, step/page/top/bottom clamps, resize keeps seen, 0/1/2-row panes see nothing, truncated batch never arms, empty batch, held `y`, held arrow still scrolls, all three approval modes, Esc and `n`, Ctrl+C, ToolResult withdraws the view | `Approvals` owns the view so answer/withdraw/new request drop it with no extra cleanup site |
| We4.2 | `E0599 no method named now for ChatView`, then 10 snapshots missing (insta `.snap.new`) | 15 of 15 | fresh, countdown, ready, scrolled, build file, coarse, too large, wrapped (`¦`), 1 and 2 rows, bidi/ESC shown as `<U+XXXX>`, the secret is shown to the user, countdown at 0/50/300/700 ms, sizes 0..=25 x {0,1,2,3,10,24,80} never panic or overdraw | `fill` in `chat.rs` made `pub(super)` |
| We4.3 | `E0425 cannot find value ASK_ARM_DELAY`; with `arm_delay` faked to 400 for all: `a_batch_waits_the_longer_review_delay` and `n_and_esc_wait_for_the_review_delay_too` FAILED | 12 delay tests | non-batch asks (Run, Read, Search, External) answer at 400 ms, batch at 800 ms for `y`, `n` and Esc | `app/tool.rs::ARM_DELAY` removed; `open_prompt` uses `ask.arm_delay()` |

Mutations (tests that passed at once): `can_approve` replaced by `true` in `approve_batch` failed 4 tests; an `AutoType` arm for `Batch` in `route` failed 2; `CHROME` 2 to 1 failed 5; a `ReviewView` `Debug` that printed rows failed the secret test; `div_ceil(100)` to `/ 100` failed the countdown test (after adding the 50 ms case).

## Deviations / notes (W-e4)

- `BatchReview` is `{ doc: ReviewDoc }` in `core/ai/chat/review.rs` with `new`, `doc`, `counts()` ("2 files, +3 -1") and the const `TOO_LARGE`. It carries no `BatchId`: W-f/W-g add it when `propose_batch` builds the ask (the pure core cannot name the runtime store).
- `ReviewView` (`core/ai/chat/review_view.rs`, new, pure): `Arc<BatchReview>` + `Coverage` + `Layout` + `top`. It lives in `Approvals` next to the pending approval, so every way an approval ends drops it. Manual `PartialEq` (same Arc, same top, same open time) and a counts-only `Debug`.
- Coverage is fed by the rows on screen: `fit(size)` runs on open, on every scroll key, on a pane resize (`App::refit_review` in the rewrap loop) and again in `y`. A jump to Home/End does not count the rows it skips; the user has to page through. The pane has `CHROME = 2` rows (counter on top, keys at the bottom); with fewer than 3 rows no body is shown, nothing is seen and `y` stays locked, while `n` still works (1 row shows the keys only, 2 rows the counter and the keys).
- Keys (`app/review.rs`): Up/k, Down/j, PgUp, PgDn, Home, End scroll (a held key repeats); `y` needs `can_approve(now)` and a Press; `n`/Esc reject; Ctrl+C denies with `cancelled` and cancels the turn; everything else is swallowed. `on_approval_key` hands off to `on_review_key` first when a view exists.
- Delays: `ASK_ARM_DELAY` (400 ms) and `review::ARM_DELAY` (800 ms) are named; `ApprovalAsk::arm_delay()` picks one and `open_prompt` sets `armed_at` from it. A batch therefore ignores `n` and Esc for 800 ms as well as `y` (a stray key cannot discard a batch the user has not seen). `Coverage::can_approve` keeps its own 800 ms check as a second guard.
- Answer to the model: `Decision::Sent` on `y`; `Denied(DECLINED)` on `n`/Esc; `Denied(TOO_LARGE)` ("the batch was too large to review: split the change into smaller batches") when the doc is not reviewable. `Decision::Denied` holds a `&'static str`, so no row text can reach it. The transcript gets `batch rejected: 1 file, +5 -0` (counts only). Apply result and paths are W-e2/W-f/W-g.
- UI: `ChatView::render_view` draws the review after the tag editor, list, note editor and summary draft (the same order in which `has_approval` hands the keys over), and `cursor()` returns `None`. `ChatView::now(Instant)` carries the clock (`ui/mod.rs` passes `app.now()`); without it the view uses `Instant::now()`. The overlay prompt (`approval_prompt`) is `None` for a batch.
- Header: `1 file, +30 -0 · seen 12/33 · 21 remaining · y locked`, then `y in 0.4s`, then `y ready`; a batch that is not reviewable shows the "too large to review" warning and the keys line `n discard · ...` only. Keys line: `y locked` / `y approve`. Build file, coarse diff and truncated warnings are the doc's own `Warning` rows.
- `cargo-insta` is not installed here: snapshots were written with `INSTA_UPDATE=always cargo test` and reviewed by eye; no `.snap.new` left.
- No macOS-only file mentions `ApprovalAsk`; `host.rs` untouched.

## Size (W-e4)

Production, counted as added lines outside test files and `#[cfg(test)]` modules: We4.1 about 380, We4.2 about 140, We4.3 about 24; total 542 (490 without blank lines). That is over the 500 cut line (under 700): We4.3 is already its own last commit, and a `size:exception` or a split is needed before opening the PR. Tests and 10 snapshots are not counted.

## Checks (W-e4)

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (5710 lib tests green), `cargo +1.88 check --all-targets`, no pending `.snap.new`.

## W-e4 review fixes (feat/coding-tools-we4)

Approved by the user: `size:exception` for this PR, and keeping `n`/Esc locked for 800 ms on a reviewable batch.

- Coverage is render-driven. `fit()` only lays out and clamps `top`. The renderer calls `ReviewView::note_drawn(now, area)` (a `Cell`); after the frame the event loop calls `App::frame_drawn()`, which runs `credit_drawn()` and credits the rows of the area and `top` that were drawn. A review in a background tab, a hidden pane or under a list/note/tag/summary view is not drawn, so it credits nothing; a resize credits nothing. `credit_drawn` returns true while the counter on screen is stale, so one more frame follows. `Coverage` gained `start(at)`; its delay starts at the first draw (`Coverage::new(doc)` has no clock, `started_at` is the test constructor).
- `n`/Esc: `ReviewView::answerable(now)` = unreviewable batch (their only exit, at once) or first draw + 800 ms. Spec scenarios added to chat-pane "Batch review view" and a line in code-write "Batch review and coverage gate".
- Redraw: `ReviewView::wake(now)` / `App::wake_review()` mark the screen dirty during the countdown and once when it ends; the 16 ms `tick` branch of the event loop calls it before `take_dirty`.
- Badges: the mode badge and the scroll badge are skipped while `ChatView::review_shown()`; they keep drawing in every other view.
- Keys row: `n reject · y approve|locked · ↑↓ PgUp PgDn scroll` (cut from the right). 9 existing snapshots changed only in that row.
- `approve()` refuses an `ApprovalAsk::Batch`; only `approve_batch` answers one. The wheel over a review scrolls the review (`Move::Up/Down`, `App::scroll_review`) and never the transcript; other mouse events do nothing.
- Tests added: first-draw crediting, hidden resize, background tab, delay from first draw, wake, n/Esc both cases, generic approve, wheel, click, Release, unfocused pane, tools off, second ask, scroll badge, mode badge, keys row at 10/19/24 (view and app level). Existing review tests now draw (a `paint`/`draw` helper credits like the loop).

RED seen: `left: 41 right: 63` (rows credited at open and on a resize while not drawn), `E0599 no method named frame_drawn / wake_review found for App`, `E0061 ReviewView::new takes 2 arguments but 3 were supplied` (the opening clock is gone), `left: [ToolDecision { .. decision: Sent }] right: []` (generic `y` approved a batch), `left: 0 right: 1` (wheel did not scroll the review), `"1 file, +5 -0 · seen 0/8 · 8 remai[+186]"` (scroll badge over the counter), `"1 file, +3 -0 · seen 6/6 · y r AUTO"` (mode badge over the tail), `left: "y approve" right: "n reject ·"` (keys row at 10).
Passed at once, proved by mutation: Release (arm made to fall through: failed), click (non-wheel mapped to a wheel tick: failed), tools off (`self.review = None` removed in `withdraw`: failed), second ask (pending check removed: failed), unfocused pane (now also asserts the shell receives the `y`).

Deviation: the event-loop wiring (`app.wake_review()`, `app.frame_drawn()` in `runtime.rs`) has no test; both methods are tested through the App. Production total of the branch against `feat/coding-tools-we3`: about 720 added lines (650 without blanks), over 500: `size:exception` as agreed.

Checks: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (5734 lib), `cargo +1.88 check --all-targets`, no `.snap.new`.

## Batch: slice W-f (host staging tools, PR 22, branch feat/coding-tools-wf, base feat/coding-tools-we4)

| Task | Status | Commit |
|------|--------|--------|
| Wf.1 `Call::{CreateDir, WriteFile, EditFile, ProposeBatch}` parse, family Write | done | feat(tools): parse create_dir, write_file, edit_file and propose_batch behind the Write family |
| Wf.2 host stages without ask or disk touch; read tools get the view; `propose_batch` seals, asks, applies on `Sent` | done | feat(tools): stage write calls in the host without a prompt and apply a sealed batch only after the user's y |
| Wf.3 toggles-off golden; Write call with Write off is `unknown tool`; host re-check | done | test(tools): pin that a write tool with its toggle off is answered like an unknown name (the host re-check landed with Wf.2) |
| Unreviewable batch | done | in Wf.2 |
| Wd-review obligation (review-only invariant) | done | host test `a_secret_in_the_base_or_the_new_text_reaches_no_model_facing_string` |

## Seam decision (W-f)

`host.rs` could not be left untouched: the batch ask must be registered before the `ToolCall` is announced and the result must go through `Owed`, so `propose_batch` has to live in `execute`'s flow. The seam is the child module `runtime/tools/host/files.rs` (all logic, 248 lines) and `host.rs` gets hooks only: +50/-23 lines (net +27).

- `TurnScope.families` (default LEGACY) and `TurnScope.stage_key` (default None); `ToolHost.stages: Arc<Mutex<Stages>>`; `ToolHost::env(origin)` replaces the inline `ToolEnv` build (so the final arm shrank by 18 lines); `checked` is `Result<Call, Cow<'static, str>>` because a seal error text is dynamic.
- Four hooks in `execute`: `files::gate` (before the ask), `files::seal` (an arm before `ask_for`), `files::answered` (first arm of the decision match), `files::run` (the old final arm: read tools over the view, and the staging calls).
- Rebase points against S1c-2 (`TurnScope`, execute filter, scope check, ask label): the two new `TurnScope` fields and their 2 lines in `TurnScope::new` and in `runtime/chat.rs:762`; the `checked` type; the `ProposeBatch` arm; the first decision arm; the replaced final arm.
- No `ToolEnv` change (12 literals would break): `tools::run_over(call, env, limits, Option<StagedView>)` carries the view; `run` is `run_over(.., None)`.
- No `BatchId` in the ask: the answer returns to the `execute` future that sealed the batch and holds the id; `Stages::apply(id, ..)` is generation-checked (`Stale` otherwise, tested). W-g needs an id route only for orphans, which have no live future.
- The app side needs no change: `Effect::ToolDecision` already reaches the approver future through `Waits::decide`, and W-e4's review produces `Sent` / `Denied(reason)`.

## TDD cycle evidence (W-f)

| Task | RED (seen failing) | GREEN | Triangulation | REFACTOR |
|------|--------------------|-------|---------------|----------|
| Wf.1 | `E0599 no variant named CreateDir / EditFile / WriteFile`, `no variant or associated item ProposeBatch` (4 errors); then `E0004 non-exhaustive patterns` in `approval.rs` and `fs.rs` | 385 core tools tests (incl. the 0.18.0 golden) | offered/parsed only with Write on (4 tools x LEGACY/on), schema bytes, argument is the path never the text, missing/wrong type/extra/not object, caps at and over (32 KiB content, 16 KiB old and new), empty `old` parses, `propose_batch` takes no key | `field()` const helper next to `text()`; `parse_built_in` handles a tool with no first argument |
| Wf.2 | `E0609 no field families / stage_key on TurnScope`, `no field stages on ToolHost` (6 errors) | 15 of 15 in `host/files_tests.rs`, 59 in `runtime::tools::host`; two RED-in-GREEN fixes of my own tests (defang is unconditional, so the family-off test stages under a key and reads with the family off; a file below a never-staged folder is refused, so the test stages the folder first) | staging asks nothing and the disk stays untouched, restage replaces, edits chain on the staged text, read/glob/grep see the pending batch and another pane does not, refusals (escape, `.env`, missing file, ambiguous edit, over the cap) stage nothing, empty propose, approved batch applied (counts and paths), reject and no approver (`Unwired`) discard, no staging while a batch waits, unreviewable batch (Sent and Denied(TOO_LARGE)), answer for a gone batch is `Stale`, drift closes the batch, family off is `unknown tool` x 4 calls x 2 sets and no key, secret test x 3 answers | `Stages::stage_with` generic over the error; `Stages` disk read wrapped as `disk_bytes` so `files.rs` names no `write`/`apply` module |
| Wf.3 | the toggles-off host test and the golden test passed at once | n/a | MCP `tools/call` of the 4 write names with all toggles off is byte-identical to an unknown name; `tools/list` does not hold them | none |

Mutations (tests that passed at once, all reverted): `gate` always `Ok` failed the family-off test; view passed whatever the family failed `with_the_family_off_the_read_tools_see_no_batch`; `Sent` ignoring `reviewable` failed the unreviewable test; no discard on deny failed 2; `made` (folder staged above) disabled failed 2; no discard after a drift failed the drift test; `{batch:?}` appended to the staging text failed the staging test and the secret test. Core: removing the family filter in `parse_built_in` failed the golden test.

## Deviations / notes (W-f)

- Result texts (counts and paths only): staging `staged <path> (batch: N of 50 entries, B of 1048576 bytes)`; apply = `ApplyResult::summary()`; reject `denied: <reason>`; unreviewable `denied: <TOO_LARGE>` (also when the answer is `Sent`: the host never applies an unreviewable batch); family off or no stage key `unknown tool`.
- `ApplyResult::applied()` added (4 lines in `apply.rs`) so the host picks the status without naming the module. `stage::disk_bytes` and `Unreadable` added; `StageFailed<E = StageError>`.
- Every denial and every non-applied apply result (`Drifted`, `Invalid`, `Failed`, `Unsupported`) discards the batch, so the model is never left with a `Proposed` batch it cannot stage into or propose again. This differs from W-e3's "stays Proposed so the user can discard it": that stays true for the store, but the host closes it because the user has answered and the model has been told. A `Partial` was already removed by the store.
- `ANOTHER_PENDING` (the app denies the ask when another approval is pending) also discards the batch: the host cannot tell it from a rejection. Rare (calls of a turn are sequential); a later slice may keep the batch.
- The batch is sealed BEFORE the ask. If the call is dropped before the answer (turn cancelled, CLI timeout, or the result slot cannot be reserved) it stays `Proposed`: that is the orphan W-g routes (`Effect::BatchReview/Apply/Discard`). Nothing here handles it.
- `create_dir` on a name that exists (file or folder) is refused with the refusal text: apply would drift on it. A file below a folder that is neither on disk nor staged is refused too (the tool descriptions do not say so yet; the model learns from `path not available`). A folder staged earlier in the batch makes the names below it absent.
- The disk read of staging is `Disk::probe` (`open_target`: checked parent, no link, nlink 1, `judge_real` on the fd path), capped at 2 MiB; `size` is the bytes read, which is enough because the batch thresholds (64 KiB, 1 MiB) are below the cap.
- Staging and apply run in `spawn_blocking` under the `Stages` mutex (poison is ignored: the store holds plain data).
- The result of a staging call is not wrapped as untrusted data: it holds only the model's own path and counts. Read results stay wrapped.
- No app test of the whole chain `y` -> `Waits` -> host -> disk: the pieces are tested (W-e4 `Sent` on `y`, `Waits::decide`, host with a scripted approver).

### Audit changes: none

`host/files.rs` names neither the `write` nor the `apply` module and holds no write token (the `create_dir` identifier is avoided: `stage_dir`, `CreateDir`). `stage.rs` is already in `MAY_CALL_APPLY`. `audit.rs` untouched; `every_tool_file_...` passes with the new file.

## Size (W-f)

About 455 production lines (core `tools/mod.rs` 105, `host/files.rs` 248, `host.rs` +48, `stage.rs` +20, `approval.rs` +7, `fs.rs` +6, `apply.rs` +5, `runtime/tools/mod.rs` +13, `chat.rs` +3) and about 880 test lines (`files_tests.rs` 543, core tests, golden). Under the ~500 cut line.

## Checks (W-f)

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (5753 lib tests green), `cargo +1.88 check --all-targets`, no `.snap.new`. No macOS-only file mentions `Call`, `ApprovalAsk`, `ToolEnv` or `TurnScope`.

## W-f review fixes

Strict TDD. Commits after the W-f slice: `test(mcp)` route family, `fix(tools)` Debug and descriptions, `test(tools)` secret coverage, `fix(tools)` drop guard, `fix(tools)` disk before lock.

| Fix | RED | Fix made |
|-----|-----|----------|
| 1 Drop guard | `a_call_dropped_while_its_batch_waits_frees_the_pane` and `a_call_that_cannot_announce_does_not_leave_its_batch_sealed`: `assertion left == right failed, left: 1, right: 0` (the sealed batch stayed in the store) | `Sealed` is an RAII guard (`Arc<Mutex<Stages>>` + `BatchId`); its Drop calls `Stages::discard(id)`. It lives to the end of `answered`, so every exit (denied, applied, error, cancel) closes the batch; the discard is generation-checked, so a stale guard is harmless. The explicit discards in `answered` were removed as redundant. The `send(announce)` failure shares the guard path with the `reserve_owned` one. W-g still owns pane-close / orphan handling. |
| 2 Lock and IO | `staging_reads_the_disk_before_it_takes_the_lock`: `the lock was held, left: Some(false) right: Some(true)`; `a_busy_store_does_not_block_the_async_worker`: `the worker is blocked on the store lock` | `stage_with_probe` reads the disk first (`probe` seam), then locks for memory only; the `made` folder check wins over the probe result. `seal` and the staged `view` take the lock inside `spawn_blocking`; the guard Drop uses `try_lock` and falls back to the blocking pool when an apply holds the store. Apply already took the lock inside its closure. |
| 3 Secret test | passed at once; mutation: appending the content to the `write_file` result failed `a_secret_reaches_no_other_model_facing_output` (reverted) | New test covers `write_file` content, staged `read_file`/`grep` (redacted), edit NoMatch/Ambiguous, Drifted, every `ApplyResult::summary()`, "waiting for review" and the events. The old test's `seen.len() >= 6` became explicit asserts per output. A plain word the model wrote itself (`hunter2`) is not redacted in the staged view: only recognizable tokens are. |
| 4 Debug | `debug_of_a_content_call_never_shows_the_text`: the derived Debug printed `content: "sk-ant-..."` | Manual `Debug for Call`: path and `content_len` / `old_len` / `new_len` only. |
| 5 MCP route | passed at once; mutation: `tools/call` forced to the Write family failed it (`expected a reply, got Call`), reverted | `a_route_without_the_write_family_lists_and_routes_no_write_tool`. |
| 6 Descriptions | `staging_tools_say_when_they_are_refused` | `create_dir`, `write_file`, `edit_file` say "Staging is refused while a batch waits for review."; `create_dir` also names the refused cases. The 0.18.0 golden is LEGACY-only: no update needed. |

The `edit_file` NoMatch / Ambiguous messages were not touched (pending product decision).

Checks: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (5761 lib tests), `cargo +1.88 check --all-targets` green; no `.snap.new`.

## W-f: edit_file asks on a Cloud engine

User decision: `edit_file` is a content oracle, so on Cloud it asks like `read_file`, once per file per turn; Local never asks.

- `ask_for`: `Call::EditFile` on Cloud gives `ApprovalAsk::Read { path }` (no new variant, no UI change); `route()` already prompts a Read in every mode, so the edit routes the same (pinned for Ask, Auto, Run and both origins).
- Host: the path is shown through `display_target` as for a read (unshowable: refused). `TurnScope.allowed: Mutex<HashSet<String>>` (per turn, since a scope is built per turn) is filled when a read or edit ask is answered `Sent`; an edit of an allowed path does not ask. A read does not consult it. A denial is not remembered and stages nothing.
- RED: `a cloud edit asks` (ask_for returned None), `[]` (no ask recorded), and a denied edit returned `staged b.txt (...)`.
- Known cost: an edit of a file the batch itself created also asks on Cloud (the host does not tell it from a disk file).

### W-f review round 2 (edit_file cloud ask edges)

- Pinned by tests: an unshowable path is refused with zero asks and nothing staged; `b.txt`, `./b.txt`, `sub/../b.txt` and `~/proj/b.txt` share one ask; a read of `./b.txt` then an edit of `b.txt` asks once, while an edit then a read asks twice (a read does not consult `allowed`); a `Busy` answer is not remembered.
- Fix: an empty `old` is refused in the host pre-check (text of `EditError::EmptyOld`) before the ask. RED: the ask was recorded for it. Missing/ambiguous `old` still asks (accepted, documented in the code-write spec together with the concurrent-edit and symlink behaviors).
- `ask_for` doc comment reflowed.

## W-g: runtime chat lifecycle (PR 23, feat/coding-tools-wg)

Strict TDD. Commits: `feat(tools)` close discards, `feat(tools)` orphans and routing, `feat(tools)` model told.

| Task | RED | GREEN |
|------|-----|-------|
| Wg.1 | `closing_a_chat_discards_its_batch_and_counts_the_entries` (compile: no `Chats::close_batch`, `discard_key` returned bool); app: close of a chat emits `BatchClose`, `AppEvent::Batch(Closed(n))` shows `discarded N staged entries` | `Stages::discard_key -> Option<usize>`; `host/orphan.rs` (`ToolHost::close_batch`); `Chats::close_batch`; `Effect::BatchClose` appended by `App::stop_effect` (close path only, so a cancel still leaves the orphan); `BatchEvent` + `closed_notice` in `core/ai/chat/review.rs`; `AppEvent::Batch` handled in `app/review.rs` |
| Wg.2 | `a_call_dropped_while_its_batch_waits_leaves_a_proposed_orphan` (`left: 0, right: 1`: W-f's drop guard discarded on cancel); `an_ended_turn_keeps_its_batch_but_a_user_no_discards_it`; `an_unproposed_batch_is_sealed_when_the_user_reviews_it`; `an_orphan_is_reviewed_applied_or_discarded_through_the_chats` | `Sealed.keep`: set by `announced()` once the ask is on the channel, so a drop while it waits keeps the batch; `answered` keeps it on `Denied(ENDED)` (turn/pane ended with the ask open: CLI timeout, cancel) and discards on any user answer. `Stages::review` over core `Batch::review_orphan(turn_live)`; `ToolHost::{review,apply,discard}_batch`; `Chats::{review,apply,discard}_batch` (`turn_live` from the pane's running task); `Effect::BatchReview(pane)`, `BatchApply{pane,id}`, `BatchDiscard{pane,id}` executed in `runtime.rs` |
| Wg.3 | `a_told_line_reaches_the_next_request_once_and_is_never_stored` (core); app `a_user_side_answer_to_an_orphan_is_told_to_the_model_on_its_next_turn`; `an_orphan_answer_never_tells_the_model_the_base_or_the_diff` (passed at once: counts only by construction) | `ChatState::tell` + `told: Vec<String>` appended to `request.system` after a blank line on the next real send (kept when a send is refused, not stored); `BatchEvent::Done { shown, told }` (told only when the batch is closed: applied/partial or discarded) |

Mutation: `keep` forced false failed `an_ended_turn_keeps_its_batch...` (reverted).

### Deviations / notes (W-g)

- **W-f drop-guard semantics changed on purpose.** The guard still discards a batch whose call never announced (closed channel). After the announce a drop keeps the batch (Proposed-orphan). `a_call_dropped_while_its_batch_waits_frees_the_pane` was replaced by the orphan test. The spec says cancel and CLI timeout leave an orphan, so this is the spec's behavior.
- A turn ended while the ask is open reaches `answered` as `Denied(ENDED)` when the call future survives (the MCP session task); it is treated as an orphan, not a user's no. Any other denial still discards.
- "System line in the request" = the chat's system text of the next request, extended by the app-side `ChatState` (not `Chats`, which must never rewrite the system prompt: `the_chats_never_rewrite_the_system_prompt`). Applied results with `Drifted`/`Invalid`/`Failed` leave the batch proposed and tell the model nothing.
- Review of an orphan is refused while a turn of the pane runs (`turn_live`), since the batch is then the model's.
- The `Review` event is only carried to the app; the view that consumes it (Ctrl+G, badge) is W-h. The apply/discard effects take the orphan's `BatchId`, so a stale id is `that batch is no longer there`.
- Apply by the user runs under the `Stages` lock on the runtime thread (one batch, at most 50 entries / 1 MiB). W-f's async path still uses `spawn_blocking`.
- `host/orphan.rs` names neither the `write` nor the `apply` module (closedness read from the store, not from `ApplyResult`): no audit change.
- Quit is not wired here (Wh.3).

### Size (W-g)

About 295 production lines (`host/orphan.rs` 84, `app.rs` ~28, `app/review.rs` ~22, `review.rs` ~26, `runtime.rs` ~27, `chat.rs` ~31, `stage.rs` ~25, `files.rs` ~25, `chat/mod.rs` ~16, `host.rs` 5) and about 450 test lines. Under budget.

### Checks (W-g)

`cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test --lib` (5776), `cargo +1.88 check --all-targets` green; no `.snap.new`.

### W-g review round

Fresh review found no critical issues; confirmed fixes, strict TDD:

- A: the early return on `Answer::Ran` leaked a Proposed orphan (RED: `a_report_answering_a_batch_denies_it_and_closes_the_batch`, staged 1). `Sealed::resumed()` clears `keep` as soon as the wait resumes, so `keep` only survives a drop by cancellation and no explicit return can leak. `RAN_ENDED` for `ProposeBatch` is a discard (`a_ran_ended_answer_discards_the_batch`).
- B: `ToolHost::apply_batch(id, turn_live)` refuses a live turn (`LIVE`) and a non-reviewable doc (`TOO_LARGE`), writing nothing and keeping the batch. `Chats::apply_batch` derives `turn_live` from the pane's task.
- C: `ApplyResult::told()` holds fixed templates with counts only (no paths); unit test over every variant. Lives in `apply.rs`, so `orphan.rs` still names no `apply` module (audit).
- D: implemented (about 17 production lines). `ChatState.told_sent` keeps what the request in flight carries; any engine event other than Slow/Notice/Error clears it; `finish` without that puts it back ahead of newer lines.
- E: drift leaves `told` None and the batch Proposed (`a_drifted_orphan_apply_leaves_the_batch_proposed_and_tells_nothing`).
- Spec notes added to code-write: accepted UI cost of the Stages lock vs. an in-flight W-f apply (50 entries, 1 MiB).

## W-h: badge, Ctrl+G, quit notice (PR 24, feat/coding-tools-wh)

Strict TDD. Commits: `feat(chat)` badge, `feat(chat)` poll and Ctrl+G, `feat(app)` quit.

| Task | RED | GREEN |
|------|-----|-------|
| Wh.1 | `badge_tests` (compile: no `Badge`, no `set_batch_badge`); `badges_count_the_entries_and_call_an_orphan_only_when_no_turn_runs` (no `Stages::badges`); `the_badges_follow_the_batches_and_the_turns_of_the_panes` (no `Chats::batch_badges`); app `the_poll_sets_and_clears_the_badge_and_redraws_only_on_a_change` | `Badge::{Staged,Orphan}(n)` + `Badge::text(room)` in `core/ai/chat/review.rs`; `ChatState.batch_badge`; `ChatView::draw_batch_badge` top left (` staged 3 `, ` orphan 2 · Ctrl+G `, then ` orphan 2 `, then ` O2 `), blue for staged and orange for orphan, hidden under the review, drawn in every other view, leaves 8 columns on the right for the mode and scroll badges; `Stages::badges(live)` (orphan = `Batch::orphan(turn_live)`), `ToolHost::badges` (`try_lock`, never waits), `Chats::batch_badges`, `App::sync_batches`, polled on the 16 ms tick in `run_loop` |
| Wh.2 | `ctrl_g_asks_for_the_review_of_an_orphan_and_nothing_else`, `the_review_event_opens_the_same_view_for_the_orphan`, `y_applies_the_orphan_behind_the_same_gate`, `n_and_esc_discard_the_orphan_after_the_delay`, `cancel_at_the_ask_then_review_apply_and_the_badge_clears` (end to end at app level with the runtime simulated by events) | Ctrl+G in `on_chat_key` (debounced like the other one-shot keys) emits `Effect::BatchReview` only for an `Orphan` badge, else a notice; `BatchEvent::Review` opens the W-e4 view as a batch ask WITHOUT a call (`Origin::Block`, `ChatState.orphan = Some(BatchId)`): same coverage gate, 800 ms arm and n/Esc rules; `y` emits `BatchApply{pane,id}`, `n`/Esc emit `BatchDiscard{pane,id}`, Ctrl+C closes the view and leaves the orphan |
| Wh.3 | app `quitting_closes_the_batch_of_each_chat_that_wears_a_badge_before_the_quit`; runtime `quitting_remembers_how_many_staged_entries_it_discarded` (no `Panes::quit_notice`) | `quit_effects` appends `BatchClose` for each chat with a badge before `Quit`; `Panes.quit_lost` is the sum of the entries lost by the `BatchClose` effects of the apply that quits, printed with `eprintln!` right after `restore()` |

### Decisions / notes (W-h)

- **Key**: Ctrl+G. The chat key table has Enter, Esc, Ctrl+C/Y/B/R/J/U, PageUp/Down and the Ctrl+Space prefix (not Ctrl+G, so no clash). Ctrl+O (S1e-1a) is not bound anywhere (modal-input spec: swallowed), so there is no conflict either. Ctrl+G was the old prefix proposal of terminal-pane, replaced by Ctrl+Space.
- **Badge data source**: staging has no event, so the runtime tick polls the store (`Chats::batch_badges`, a `try_lock` of the memory-only store) and the app compares. A model's staging shows up within one tick (16 ms).
- **Staged vs orphan**: orphan = the batch has entries and no turn of its pane runs (the same rule `review_orphan` applies), so the badge and the host never disagree.
- **Orphan review reuses the W-e4 view as a call-less batch ask**: `reviewed_orphan` requires `approval.call().is_none()` AND a stored orphan id, so a stale id can never turn a model's `y` into an apply by id (test pinned). `close_approval` clears the id and no longer writes "cancelled" for a batch review (the `Done` notice speaks).
- **Quit notice** can only be shown after the terminal is restored (the panes are gone at that point), so it is a stderr line: `discarded N staged entries`. Quits by signal do not print it (no `BatchClose` ran); the batch is memory only either way.
- Quit only closes batches of chats that wear a badge (the 16 ms poll); a batch staged in the last tick before the quit is still lost but not counted.
- README key table (Spanish) not touched; add the Ctrl+G row in the docs slice.

### Size (W-h)

About 255 production lines (`review.rs` 81, `chat/mod.rs` 28, core `review.rs` 28, `ui/chat.rs` 29, `runtime.rs` ~20, `orphan.rs` 21, `stage.rs` ~20, `runtime/chat.rs` 12, `notes.rs` 7, `run.rs` 5, `chat.rs` 2) and about 500 test lines. Well under budget.

### Checks (W-h)

`cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test --lib` (5805), `cargo +1.88 check --all-targets` green; no `.snap.new`. Three new snapshots reviewed (`staged_badge_40x5`, `orphan_badge_40x5`, `orphan_and_auto_badges_40x5`).

### W-h review round

A fresh review found nothing critical. Six confirmed items, one commit each:

1. **Badge flicker**: `ToolHost::badges` and `Chats::batch_badges` return `Option<HashMap>`; `None` on a busy store (`WouldBlock`) or without tooling. `run_loop` calls `sync_batches` only on `Some`, so a contended poll keeps the previous badges. Tests: `a_contended_badge_poll_answers_none_and_a_free_one_answers` (host holds the `Stages` lock), `None` without tooling in `the_badges_follow_the_batches_and_the_turns_of_the_panes`.
2. **Quit discards every chat**: `quit_effects` emits `BatchClose` for every chat pane (no badge filter; `discard_key` is a no-op for an absent or empty batch), so a batch staged since the last poll is also discarded and counted. Test: `quitting_closes_the_batch_of_each_chat_before_the_quit`; six older quit expectations now include the `BatchClose`.
3. **Badge no longer hides the first transcript row**: the conversation reserves its top row for the badge (`ChatView::plan` shrinks the transcript by one row from the top; `badge_row`). Chosen over the composer border because the badge already lives top left with the mode badge on the same row, and the border is the composer's own chrome, drawn in other places. A pane whose transcript has fewer than 2 rows, or too narrow for any badge text, reserves nothing and draws no badge. The list, note, tag and summary views keep the overlay (they have no transcript to protect). Three snapshots regenerated: `› question` is now visible; no other snapshot changed. Tests: `the_batch_badge_keeps_the_first_transcript_row_visible`, `a_pane_too_small_to_spare_a_row_shows_the_transcript_and_no_badge`, `the_layout_gives_up_a_row_only_while_a_badge_is_drawn`.
4. **Quit notice**: `writeln!(io::stderr(), ..)` with the result ignored instead of `eprintln!`.
5. **Ctrl+G on a stale badge**: when `review_batch` returns `None` with tooling present, the runtime answers with `BatchEvent::Done { shown: NO_ORPHAN }` (the constant moved to core `review.rs` and is shared with the app). The 16 ms poll clears the badge on the next tick. Test: `a_review_of_a_stale_badge_answers_with_a_notice`.
6. **Doc only**: signal exits print no discard notice (see code-write spec, "Notes: orphan answers").

Note: the W-h note above says quit only closes badged chats; superseded by item 2.


## Batch 16: slice M5-a (docs for search and write, PR 25, branch feat/coding-tools-m5a, base feat/coding-tools-wh)

| Task | Status | Commit |
|------|--------|--------|
| M5a README: search, write, toggles, review keys, Ctrl+G row, threat model | done | docs(readme) |
| M5a CHANGELOG `[Unreleased]` entries for write | done | docs(changelog) |

Notes:
- README is mostly Spanish (a few older English subsections); the new section is neutral Spanish. It sits before "Tools with Ollama" as `### Herramientas de búsqueda y escritura`; Ctrl+G row added to the chat key table; the Ollama round cap sentence now mentions 24.
- CHANGELOG: search already ships in 0.23.0 on main (toggles, glob, grep, read_file paging), so only write goes under `## [Unreleased]` (Added, Changed). No version header, no compare links.
- Verification: `cargo test --doc` ok (0 doc tests); no test reads README or CHANGELOG; no links added.
- Inconsistencies: spec "Root policy" says the origin is refused only for HOME/ancestor/system/missing, design D12 and code also refuse an origin outside HOME (docs follow D12). The `closed_notice` text is `discarded N staged entries`. The `staged N` badge is shown while the turn is live and `orphan N` once it is gone (spec says orphan is for a cancelled turn only).


## Batch 17: slice X-a (allow-list, PR 15, branch feat/coding-tools-xa, base feat/coding-tools-m5a)

| Task | Status | Commit |
|------|--------|--------|
| Xa.1 allow table per program and verb, denied flags | done | feat(tools): allow only six programs, their verbs and no config, path or escape flags for run_command |
| Xa.2 paths, cwd, `parse_run` with timeout clamp | done | same commit (one file) |

TDD evidence: RED = `E0425 cannot find function check`, `E0433 Program/Refusal` (41 errors), then `E0425 parse_run`, `RunError`, `RunRequest` (13). 13 table tests green at once, 19 after Xa.2; one test expectation fixed (`x:/y` is a drive path, refused on purpose). Mutations: install accepts packages, backslash split removed (survived until `..\x` tests were added), `=value` check removed, default 121: each failed a test; `+ 0` equivalent mutant survived.

Notes:
- `core/ai/tools/allow.rs` (pure). API: `Program`, `Refusal{Program,Verb,Flag,Path}` with fixed `text()` ("path not available" for Path), `check(program, &[String]) -> Result<Program, Refusal>` (order: program, verb, flags, paths), `RUN_ARGS` (ArgDef table: program, args up to 64 x 512 B, cwd, timeout_secs), `parse_run(&Value) -> Result<RunRequest{program,args,cwd,timeout}, RunError{Shape(ToolError),Refused}>`, `TIMEOUT_DEFAULT` 120, `TIMEOUT_MAX` 600.
- DECISIONS: flags are judged only before `--` (for go also before `-args`): what follows belongs to the program run. Paths are judged in every arg, in the word, the `--flag=value` value and the value glued to a short flag (`-c/etc/x`); `/`, `\` and `C:\` starts are absolute; `..` is a path part, so `a..b` passes. Timeout is CLAMPED (1..=600, 0 and negatives become 1, huge values 600). `cwd` is judged by text only (`plain_relative`); the real-disk root check stays in X-d1. `install`/`ci` refuse any non-dash arg (package args), `run` needs a script word. `cargo fmt` needs `--check` anywhere. Extra denied flags beyond the design table: cargo `--allow-staged`, `--allow-no-vcs`; node `--userconfig`, `--globalconfig`, `--cwd`, `--dir`, `-C`; go `-overlay`, `-vettool`, `-C`.
- `RUN_ARGS` is not yet a `ToolDef` in the registry and `Call::RunCommand` does not exist: X-d1 wires them. Nothing in runtime/, host.rs or audit.rs touched.
- Size: ~290 production lines (267 non-blank), ~330 test lines. Checks: fmt, clippy -D warnings, cargo test --lib (5829), +1.88 check --all-targets.

Review fixes (slice X-a):
- USER DECISION: `yarn run <x>` / `pnpm run <x>` only start a script declared in `package.json` `scripts`; allow.rs stays pure, so `check`/`parse_run` take `scripts: &[String]`. TODO(X-d): read `package.json` at the call site and pass the names. npm `run` is not gated (npm refuses an unknown script itself).
- Node flags: also deny `--location`, `--registry`, `--config`/`--config.*`, `--store-dir`, `--global-folder` and any short bundle containing `g` (`-gD`).
- `path_like` judges every `=`-separated tail, and the glued short-flag value of each tail (`-ldflags=-extld=/bin/sh`, `a=b=../c`).
- pytest: any word starting with `@` is refused (argparse expands argsfiles also after `--`).

## Batch 18: slice X-b (Capture and RunOutcome, PR 16, branch feat/coding-tools-xb, base feat/coding-tools-xa)

| Task | Status | Commit |
|------|--------|--------|
| Xb.1 `Capture` ring (4 KiB head, tail, marker) | done | feat(tools): capture a run as a 4 KiB head and a tail ring and report it as a cleaned, redacted RunOutcome |
| Xb.2 `RunOutcome`, digest, `text()`, `render()` | done | same commit (one file) |

TDD evidence: RED = `E0433 cannot find type Capture / RunOutcome / Ended`, `E0425 HEAD / TAIL / CAP / DIGEST` (60 errors). 20 tests green at once after the implementation (no failing GREEN; the fillers of two tests were changed BEFORE implementing because `redact` takes long single-character runs for secrets). Mutations: tail not capped per chunk (equivalent mutant: the drain gives the same bytes), no redact, no clean, no char-boundary cut in the digest, tenths of second as hundredths, omitted forced to 0, head disabled: each failed 1-5 tests.

Notes:
- `core/ai/tools/run.rs` (pure). API: `Capture::{new, push, total, truncated, bytes, text}`, `Ended{exit, signal, timed_out, cancelled}` (Default), `RunOutcome{argv, exit, signal, timed_out, cancelled, truncated, total_bytes, duration, digest}` per design, `RunOutcome::{new(argv, Ended, &Capture, Duration), render(nonce, &Capture)}`. Constants `CAP` 32 KiB, `HEAD` 4 KiB, `TAIL`, `DIGEST` 2 KiB.
- DEVIATION (small): `TAIL` is 28 KiB minus 64 bytes (`MARKER_ROOM`), not exactly 28 KiB, so head + marker + tail is at most 32 KiB and `label::wrap` (cap 32 KiB) never cuts the tail off. Marker is `\n[... N bytes omitted ...]\n`.
- Decisions: `text()` = lossy UTF-8 of the kept bytes, then `clean`, then `redact` (the read tools' order); a character cut by the ring becomes U+FFFD. The digest is the last <= 2048 bytes of that text on a char boundary, so it is cleaned and redacted too. `render` returns one facts line (`exit code 101 after 2.4s`, `killed by signal 11 ...`, `timed out ...`, `cancelled ...`, plus `; output cut, N bytes in all`) followed by `label::wrap(nonce, text)`; the facts line carries no model-supplied text (argv is not echoed), and an empty output is `(no output)`. Durations show truncated tenths of a second. Pathological output (invalid UTF-8 growing 3x as U+FFFD) may still hit the wrapper cap; accepted.
- Nothing wired: no `Call`, no runtime, no audit change; no `create_dir` identifier.
- Size: ~170 production lines (150 non-blank), ~280 test lines. Checks: fmt, clippy -D warnings, cargo test --lib (5849), +1.88 check --all-targets.

Review fixes (slice X-b):
- `Capture::text()` decodes, cleans and redacts head and tail separately when the output was cut, then joins them with the omitted marker, so an unterminated OSC/DCS/APC in the head cannot swallow the marker or the tail. An uncut output stays one stream, so a secret straddling the 4 KiB boundary is still redacted.
- The size budget now applies after decode + redaction (U+FFFD is 3 bytes for 1): the leading side of the tail is trimmed to fit `label::OUTPUT_CAP`, with a `[... output shortened ...]` note, so `label::wrap` never drops the end. Tests: unterminated OSC at the head boundary, 0xFF-heavy output (cut and uncut), secret across the boundary.

## Batch 19: slice X-c1 (toolchain JailSpec, PR 17, branch feat/coding-tools-xc1, base feat/coding-tools-xb)

| Task | Status | Commit |
|------|--------|--------|
| Xc1.2 `JailSpec.rwx`, `check_exec` (R29 amended), SBPL refuses `rwx` | done | feat(jail): let a JailSpec carry one rwx root and refuse it on macOS |
| Xc1.1 `core/ai/toolchain.rs` `toolchain_spec` | done | feat(jail): build the toolchain jail spec from a canonical program path and a fake-able file system |
| Xc1.3 `toolchain/env.rs` `scrub` | done | feat(jail): scrub the environment of a toolchain run down to a short list and a clean PATH |

Order: Xc1.2 first (Xc1.1 builds on `rwx`).

TDD evidence: Xc1.2 RED = `E0560 no field rwx`, `E0599 no method check_exec / exec_paths`, `E0433 ExecFault`, `no variant SbplError::Rwx` (22 errors); 8 tests green at once. Xc1.1 RED = `cannot find Layout / Refusal / Vars / toolchain_spec` (13 errors); 12 tests, 3 failed on MY expectations (cache order, `.npm` listed for every tool, a GOROOT that does not contain the program is correctly refused): tests fixed, code untouched. Mutations: Never check without the "over" direction failed 1; InRoot always-true failed 1. Xc1.3 RED = `E0425 cannot find function scrub`; 5 green after fixing one test (single-letter secret values matched the Debug text). Mutations: PATH root filter off and relative filter off each failed 2.

Notes:
- macOS DECISION: refuse, do not render. `JailSpec::check_exec(Os::Macos)` gives `ExecFault::Macos` for any `rwx`; `sbpl::render` gives `SbplError::Rwx`. The macOS overlap check `writable_executable` never sees `rwx`. CLI specs never carry `rwx` (test on Linux/Macos/Other). This branch does NOT have 0.25.1 (`rw_except`, literal pins): rebase point, `rwx` must be added next to them in `sbpl::render` count and classes when the chain meets main.
- API: `JailSpec.rwx`, `exec_paths()`, `check_exec(os)`, `ExecFault{ManyRwx,NotCanonical,TooBroad,Macos}`; `jail::broad_place(path, homes)` (too_broad for all three CLIs, Linux rules), `LINUX_RO*`, `DEV_*`, `ETC_LINKS`, `push_unique`, `is_canonical_form` now `pub(crate)`. `toolchain::{Layout{tool: Program, home, root, program, tmp, vars}, Vars{cargo_home, rustup_home, goroot}, Refusal{NotCanonical, Root, InRoot, Program, Never}, toolchain_spec(&Layout, resolve)}`; `toolchain::env::scrub(read, root, tmp) -> Vec<(OsString, OsString)>`.
- The spec has no `rw_files` for tmp; caches (`~/.npm`, `~/.cache/{pip,yarn,go-build,pre-commit}`, `~/go/pkg/mod`, `~/.local/share/pnpm`, `$CARGO_HOME/{registry,git}`) are added only when `resolve` finds them, for every tool. `GOCACHE`/`GOMODCACHE` env are passed through but do not move the cache grants (open).
- `JailArgs::render` and Landlock `rules()` ignore `rwx` until X-c2 (fewer rights, never more). X-c2 must add `--rwx` render/parse and a Landlock rule with execute for it, and call `check_exec`.
- X-c3 needs: `scrub` output as the child env, a `resolve` backed by `fs::canonicalize`, PATH search over the scrubbed PATH to get `Layout.program`.
- Open questions in design.md: macOS run_command (needs a Seatbelt toolchain slice, user decision), pytest `--user` site-packages read access.
- Size: ~375 production lines (jail.rs 63, sbpl 6, toolchain.rs 236 incl. 3 doc lines, env.rs 69, mod 1), ~455 test lines. Checks: fmt, clippy -D warnings, cargo test --lib (5874), +1.88 check --all-targets.

Review fixes (slice X-c1):
- R29 on Linux: `writable_executable` is `pub(crate)` and also walks the `rwx` root; `toolchain_spec` drops any cache equal to, under or over an execute place (Go `~/go`, an `rwx` root over a cache) and refuses anything else writable there as `Refusal::WritableExec`; `JailSpec::check_exec(os, homes)` returns `ExecFault::WritableExec` too. The Docker rust layout is already `Refusal::Never("/usr")` (credentials inside the system `rx`).
- `go_root` is skipped when `rx` covers the program (`/usr/bin/go`, `/usr/local/go/bin/go`).
- `check_exec` takes `homes` and rejects a broad root with `broad_place`.
- design.md: D24 row moved under D23; X-c3 notes (exec-capable GOTMPDIR, cargo `.package-cache`/`.global-cache`, git integrity check USER DECISION, `.env` in the root accepted residual risk).

## Batch 20: slice X-c2 (helper flags, PR 18, branch feat/coding-tools-xc2, base feat/coding-tools-xc1)

| Task | Status | Commit |
|------|--------|--------|
| Xc2.1 `--rwx` render/parse, Landlock rwx rule with Execute, `check_exec` in the helper | done | feat(jail): let the helper take an rwx root and grant it execute under Landlock |
| Xc2.2 `--net-bind open`, `--rlimits toolchain` (CORE 0, FSIZE 4 GiB, CPU 1200 s) | done | feat(jail): let the helper open the TCP bind and cap core, file size and CPU time for a toolchain run |
| UNIX socket residual risk check | done | docs commit (design.md, this file) |

TDD evidence: Xc2.1 RED = pure round trip failed (`--rwx` unknown flag path) and two real-helper tests failed (rwx exec denied, two rwx roots not refused). GREEN: both green, 17 tests in `tests/jail.rs`. Mutation: rwx rule with `read_write_dir` (no Execute) failed the rwx exec test. Xc2.2 RED = `E0433 NetBind / Rlimits / Limit`, `E0560 no field net_bind / rlimits` (9 errors); GREEN at once, helper tests green on this kernel (ABI >= 4, so none skipped). Existing struct literals of `JailSpec` in two unit-test files needed `..JailSpec::default()` / the new fields.

Notes:
- API (core/ai/jail.rs): `JailSpec.{net_bind: NetBind, rlimits: Rlimits}` (Default = Closed / None), `Rlimits::limits() -> &[(Limit, u64)]` with `Limit::{Core, FileSize, CpuSeconds}`, constants `RWX_REASON`, `RLIMIT_REASON` (on the `known_helper_reason` whitelist), two new `ARG_*` reasons. The flags live on `JailSpec`, not `JailArgs`, so no existing `JailArgs` literal changed.
- Helper (runtime/jail.rs, Linux): `check_exec(Os::Linux)` fails with `NotEnforced(RWX_REASON)` (exit 125, program never runs); `set_limits` runs before `enforce` (soft = hard); the rwx root gets `AccessFs::from_all(abi)` (Execute included); BindTcp is handled only when `net_bind` is Closed and ABI >= 4.
- macOS: `sbpl::render` refuses rwx, an open bind and toolchain rlimits with `SbplError::Rwx` (fail closed; no macOS toolchain slice planned).
- `toolchain_spec` (X-c1) does not yet set `net_bind`/`rlimits`: X-c3 sets them on the run spec (net_bind Open only if a dev server is wanted; rlimits Toolchain always).
- Residual risk (checked): pathname UNIX socket `connect()` is not mediated up to Landlock ABI 8. ABI 9 adds `LANDLOCK_ACCESS_FS_RESOLVE_UNIX` (landlock crate 0.4.7 `AccessFs::ResolveUnix`), already handled by `from_all` in the helper, so ABI 9 kernels gate it. The local 6.19 kernel headers lack it (ABI <= 8 here). Accepted; seccomp AF_UNIX filter is a follow-up. A stale comment in `tests/jail.rs` claiming the crate caps below ABI 9 was corrected (the crate supports V9).
- `ulimit -f` is 512-byte blocks in dash and 1024 in bash, so the rlimit test accepts either value for 4 GiB.
- Size: ~125 production lines (jail.rs 81, runtime/jail.rs 38, sbpl.rs 6), ~255 test lines. Checks: fmt, clippy --all-targets -D warnings, cargo test (5880 lib, 19 jail).


Review fixes (slice X-c2):
- Helper `check_rwx`: `check_exec(Os::Linux, homes)` with `$HOME` (both spellings) plus a `fs::canonicalize(root) == root` test, so a symlinked or broad `rwx` root is refused with `RWX_REASON`. Integration fixtures now live under `CARGO_TARGET_TMPDIR` because `/tmp` is a broad place.
- `set_limits`: `min(getrlimit hard, value)` (a lower parent hard limit no longer gives EPERM); an out-of-range value is `RLIMIT_REASON`, never `RLIM_INFINITY`.
- Tests: the rlimit test drops the inherited `ulimit -c` baseline, a lowered parent hard limit and symlinked/broad roots are end-to-end, `RLIMIT_REASON` is in the safe-reasons vector.
- design.md: ABI bullets list restored (the X-c3 notes now follow it), helper rules and the in-root secrets/hooks residual risk stated.

## Batch 21: slice X-c3 (runner, PR 19, branch feat/coding-tools-xc3, base feat/coding-tools-xc2)

| Task | Status | Commit |
|------|--------|--------|
| Xc3.1 spawn core: `std::io::pipe` shared by stdout and stderr, stdin `/dev/null`, `process_group(0)` + `GroupKill`, audit exemption for `runner.rs` | done | feat(tools): spawn a program on one shared pipe in its own process group |
| Xc3.2 timeout, cancel by drop, group kill of grandchildren, `kill_and_reap` then 500 ms drain | done | feat(tools): kill the process group on timeout, on cancel and after a normal exit |
| Xc3.3a `Scratch` (private TMPDIR / GOTMPDIR) + `integrity` snapshot/compare | done | feat(tools): keep a private folder per run and watch git hooks, config and submodules |
| Xc3.3b `run_jailed`: scrub, profile, GOTMPDIR, integrity, ring | done | feat(tools): run a toolchain program through the jail with a scrubbed env and watched git places |

TDD evidence: Xc3.1 RED = `E0422/E0425 cannot find Run / Done / spawn_core` (7 errors), 9 tests green; audit RED = `runner.rs: path segment process/net, Command, libc::pid_t, libc::ETXTBSY` until the spawn side was added. Xc3.2 RED = timeout test never ended (no deadline; had to be killed by pid) and a setsid test; GREEN 16. Mutations: normal exit with `reap` instead of `kill_and_reap` hung the grandchild test; `DRAIN` 500 -> 5 ms failed the direct drain test; removing the `if open` drain alone SURVIVED every end-to-end test (the exit wake takes a `spawn_blocking` round trip, so the data is nearly always read first): the drain is covered by a direct test on `drain` (bytes left, writer still open, bound reached), not by a race. Xc3.3a RED = `E0425 snapshot/compare/Change/Kind/TEXT_CAP`, `E0433 Scratch` (23); 16 green; mutations: mode ignored, `metadata` instead of `symlink_metadata`, text kept over cap, digest dropped, scratch mode 0o755: each failed one test. Xc3.3b RED = `E0425 run_jailed/profile`, `E0422 Job`, 16 errors; 26 green; mutations: parent env leaked in, GOTMPDIR for the wrong tool, `profile` skipped, `compare(before, before)`: each failed 1-2 tests.

API (`runtime/tools/runner.rs`): `Run{program, argv, cwd, env, timeout}` + `spawn_core(Run) -> io::Result<Done{ended: Ended, capture: Capture, duration}>` (below the allow-list; `timed_out` set, `cancelled` never: cancel is a dropped future). `Job{helper, spec, tool, program, argv, root, cwd, scratch: Scratch, timeout}` + `run_jailed(Job, &read_env) -> io::Result<Ran{done, changes: Vec<Change>}>`; `profile(spec)` sets `NetBind::Open` and `Rlimits::Toolchain` (always: a dev server may listen). X-d1 builds the `Layout` AFTER `Scratch::unique(parent, "run")` so `Layout.tmp == job.scratch.path()`, then `toolchain_spec`, then `Job`, then builds `RunOutcome::new(argv, done.ended, &done.capture, done.duration)`.

Audit: `MAY_SPAWN = ["runner.rs"]` (`Command` and a `process` path segment, `libc::{pid_t, ETXTBSY}`, and `tokio::net::unix::pipe` only; sockets, `FS_WRITE`, other libc stay banned) and `scratch.rs` in `MAY_WRITE` (it makes and removes folders; `getpid` instead of `std::process`). New test `the_runner_may_start_a_process_and_nothing_else_and_others_may_not`.

Integrity (user decision 5B): `integrity::{snapshot, compare, Change{path, kind: Added|Changed|Removed, before, after, mode}}`. Not run through a link; `.git` counts when it is a file; hooks capped at 64 names; text kept <= 64 KiB, bigger files by sha256. X-d1 MUST surface `Ran.changes` (warning with a diff built from `before`/`after`, and an offer to revert from `before`). Nothing writes in X-c3. A change is reported even when the run times out or fails.

Manual checks (throwaway test, removed): a real `cargo check` in the real jail with a populated registry and with an index update: `.package-cache` needs no `rw_files`; `.global-cache` prints a harmless last-use warning (SQLite journal not writable; accepted), see design.md. Cargo's workspace search fails when an ancestor of the root holds an unreadable `Cargo.toml`.

OPEN for X-d1 (risk): the helper execs with `arg0 = program` and `Layout.program` is the CANONICAL path, so `~/.cargo/bin/cargo` (a rustup proxy) becomes `rustup` and would run as rustup, not cargo. By reading the code only (not run): X-d1 must pass the proxy name as arg0 or resolve the real toolchain binary. `toolchain_spec` only accepts cargo under `$CARGO_HOME/bin`, so the toolchain-dir binary is refused today. My manual check used `rustup run stable cargo ...`.

Size: see report. Checks green: fmt, clippy --all-targets -D warnings, cargo test.


## X-c3 review fixes

Eleven confirmed review findings fixed on `feat/coding-tools-xc3` (strict TDD: each test seen red, by a failing assertion or a compile error on the new API, before the code).

| # | Fix | Where |
|---|-----|-------|
| 1 | Hooks are never capped silently: every entry is read; more than `HOOKS_ABSURD` (1024) in one folder gives a `Kind::Crowded` change on every run (`after` = "hooks dir has N entries") | `integrity.rs` |
| 2 | `.git` is judged with `symlink_metadata` first: a link or a file is recorded and never entered; every read goes through `open_beneath` (openat2 BENEATH + NO_SYMLINKS, O_NOFOLLOW walk as fallback) | `integrity.rs` |
| 3 | Watch list adds `core.hooksPath`, `include.path` / `includeIf.*.path` (chains, depth 6) when they resolve inside the root, `.git/modules/**/{config,hooks/*}` (nested module names too), `.git/config.worktree`, and the `gitdir:` target of a `.git` file when inside the root | `integrity.rs` |
| 4 | `Scratch::unique` names are pid + counter + nanos and a taken name is skipped (16 tries, then `AlreadyExists`) | `scratch.rs` |
| 5 | A failed `remove_dir_all` chmods the tree's folders (never links) to 0700 and retries once; what is still there is returned (`Scratch::finish`, `Ran.leftovers`) | `scratch.rs`, `runner.rs` |
| 6 | `AfterOnDrop` guard holds `before`; a dropped or failed run takes a synchronous after-snapshot and pushes the diff to `Job.unreported` (`Unreported`) | `runner.rs` |
| 7 | `NetBind::Open` kept; decision and port-squatting risk recorded in design.md (resolved list, D15) | `design.md` |
| 8 | `JailArgs.arg0` / `--arg0 <name>`: plain name only (no `/`, no NUL, not empty); the helper runs `Command::new(canonical).arg0(name)`; `run_jailed` passes `Program::name()`. Linux only (sandbox-exec starts the program under its path) | `core/ai/jail.rs`, `runtime/jail.rs`, `runner.rs` |
| 9 | The final `kill_and_reap` is bounded at 5 s (`REAP`) and is an error when it is not met | `runner.rs` |
| 10 | `scratch.rs` has its own narrow allow list (`DirBuilder`, `remove_dir_all`, `set_permissions`, libc `getpid`); `DirBuilder` joins `FS_WRITE` (it was not banned anywhere); an aliased `fs` stays banned there | `audit.rs` |
| 11 | `safe_path` canonicalizes each PATH entry before the in-root check (the entry is kept as written) | `core/ai/toolchain/env.rs` |

Tests: the timeout test uses a ready marker (5 s deadline instead of 400 ms) and checks the leader is reaped; a Go dropped-run test (GOTMPDIR removed, group killed, leader reaped); a real-helper test that a `bash` started with `--arg0 cargo` reads `cargo` in `/proc/self/cmdline`.

API for X-d1:
- `Ran { done, changes: Vec<Change>, leftovers: Vec<PathBuf> }`. Surface `changes`; a `Kind::Crowded` change means "too many entries to see": treat the run as tainted, no diff, no revert. Say so when `leftovers` is not empty.
- `Job.unreported: Unreported` (shared handle, `Clone`): keep a clone, call `unreported.drain()` after a cancelled run and treat the result like `Ran.changes`. A run that finishes never pushes.
- `arg0` is set by `run_jailed` from `Job.tool.name()` (`cargo`, `go`, ...): X-d1 passes `Layout.program` as the canonical path and needs nothing else. This closes the OPEN risk of Batch 21 (`~/.cargo/bin/cargo` canonicalizing to rustup). `$RUSTUP_HOME` stays in `rx`.
- Limit: the drop snapshot is taken right after the group is signalled, so a write in the instant between the signal and the death can be missed.


## X-c3 review fixes, round 2

Principle (orchestrator decision): the integrity check FAILS CLOSED. Git's config parser is not re-implemented: the plain subset is read exactly, and anything else marks the run as tainted. Strict TDD: the new tests were run against the previous code first (20 integrity tests red; the deep-tree scratch test aborted with a stack overflow; the drop test timed out; the env test failed) and then went green.

| # | Fix | Where |
|---|-----|-------|
| 1 | Configs are read for parsing with their own cap (`CONFIG_CAP` 1 MiB), not `TEXT_CAP`; over it the run is unchecked | `integrity.rs` |
| 2 | Strict-subset reader: `[core] hooksPath = x` on one line, `[Core] HooksPath` (case-insensitive), `[section "sub"]`, `[section.sub]`, `#`/`;` comments, a quoted value. Tainted: a backslash continuation anywhere, a stray CR or NUL, invalid UTF-8, a header it does not understand, inner or unbalanced quotes or a backslash in the value of a key it reads, `~` or `%(prefix)`, an empty path, a bare `hooksPath` | `integrity.rs` |
| 3 | Include and submodule depth 6 -> 10 (git's limit); deeper is unchecked | `integrity.rs` |
| 4 | A watched folder (`.git/hooks`, `.git/modules`, a module entry, a `hooksPath` folder) that is a link or a file is recorded as an item; a link to a folder inside the root is followed, so its hooks are watched | `integrity.rs` |
| 5 | `core.hooksPath` equal to the root (`.`, or the root's absolute path) watches git's hook names in the root; a `hooksPath` folder that does not exist yet is watched through its lexical path | `integrity.rs` |
| 6 | A linked hook or config also records its target when it resolves inside the root; a linked config is parsed from the target content (relative includes tried against both folders); a config link that leads outside the root is unchecked | `integrity.rs` |
| 7 | A relative `core.hooksPath` is resolved against the git dir's `core.worktree` when set; for a submodule git dir with no `core.worktree` it is unchecked; a worktree outside the root leaves nothing to watch | `integrity.rs` |
| 8 | Bounds per snapshot: 4096 items and 256 MiB hashed (`Limits`, `snapshot_with` for tests); past them the rest is not read and the run is unchecked. Folders are counted while iterating (stop collecting past the threshold, stop counting at 100000) | `integrity.rs` |
| 9 | `make_removable` is an explicit-stack walk (`walk`), bounded to depth 4096 and 1,000,000 entries | `scratch.rs` |
| 10 | The drop-time snapshot and the scratch cleanup of a dropped run run on detached `std::thread`s; the finished-run path takes its after-snapshot and removes the folders in `spawn_blocking`. `Unreported` may fill shortly AFTER the drop | `runner.rs` |
| 11 | `safe_path` canonicalizes `root` once | `core/ai/toolchain/env.rs` |

Notes: a path cannot be longer than `PATH_MAX` (4096), so the deep-tree scratch tests use 1500 levels (about 3000 characters) on a 64 KiB thread stack, where one frame per level would overflow, instead of 5000 levels. Items 4-6 resolve a path by `canonicalize` when it exists and by a lexical walk that refuses to cross a link when it does not.

API changes for X-d1:
- `Kind::Crowded` is now `Kind::Unchecked` (still `Copy`): `after` holds the reason text ("hooks dir has N entries", "config not understood: line 3: line continuation", "more watched places than the bound allows", ...). Show it as "could not verify git hooks/config after this run", with no diff and no revert. It is reported on every run, whatever changed, and also when only the before-snapshot was unchecked.
- `Unreported` may fill shortly after a cancelled `run_jailed`, not at the drop: drain it again later (before a commit or push). If the thread cannot start, a `Kind::Unchecked` change is pushed.
- No other signature changed (`snapshot`, `compare`, `Change`, `Ran`, `Job`).
## Batch: slice G-a (git argv and parse, PR 20, branch feat/coding-tools-ga, base feat/coding-tools-m5a)

| Task | Status | Commit |
|------|--------|--------|
| Ga.1 argv templates (status, diff, log, add, commit), `-c` hardening, `:(exclude,glob,icase)` from SECRET_FILES, `:(literal)` | done | feat(tools): add pure git argv templates and porcelain/log parsing |
| Ga.2 porcelain v2 `-z` parse, log `%x1f` parse, `max_count` <= 50 | done | same commit |

Notes:
- `core/ai/tools/git.rs` (pure, about 390 production lines with docs). `SECRET_FILES` in `paths.rs` is now `pub(crate)` (one-word change); `pub mod git;` in `tools/mod.rs`.
- Strict TDD: tests and code were written in one pass in one file, not as a separate red run; one test expectation was wrong and fixed at the first run.
- Deviations: pathspec is `:(exclude,glob,icase)` (spec says `exclude,glob`) so a tracked `.ENV` is excluded like the case-insensitive path guard; `.env` and `.env.*` are added to SECRET_FILES because the guard treats env names separately. `git_diff`/`git_log` path is `:(literal)` and checked (relative, no `..`, no `.git`, no secret component). `git log` with a path appends excludes too. Default log count 20 (design leaves it open).
- Pure helpers also in the module: `env(read)`, `check_message` (non-empty, <= 4096 bytes, no control except `\n`/`\t`, no bidi), `render_status`, `render_log`, status entries cap 200.
- Checks: fmt, clippy -D warnings, `cargo test --lib` (5828), `cargo +1.88 check --all-targets` green.

### G-a review round (feat/coding-tools-ga; commits 7b38111, 54ed578)
- C1: `add_argv` appends `excludes()` (a folder or `.` no longer stages secrets); temp-repo test (skips without git) pins it.
- Hardening: `--no-show-signature`, diff `--ignore-submodules=all`, `-c gpg.program=false`, env `GIT_ATTR_NOSYSTEM`, `GIT_CONFIG_SYSTEM=/dev/null`, `GIT_CONFIG_GLOBAL=/dev/null`; log always has `--`; commit `--cleanup=whitespace`.
- User decisions: `GitDir` (absolute clean path) is a parameter of every builder and pins `core.hooksPath=<git-dir>/hooks`; `hooks_notice(dir, configured)` for the commit review; `Identity` (name, email) gives `-c user.name/user.email`, validated. G-b: read the identity from the global config in the preflight, read `core.hooksPath`, resolve the git dir.
- Parse/render: rename orig record always consumed (no forged entry); entries with a secret path/old path/secret folder dropped; `tidy` replaces control/format chars with `?` in rendered text. Secret folders (SECRET_DIRS, CREDENTIAL_DIRS) excluded as `**/<dir>/**` and refused by `check`. `.claude/` is in CREDENTIAL_DIRS, so a repo's own `.claude/` is excluded from add/diff/status.
- TDD: tests first for the review items (7 RED), then the identity/hooks tests (RED = compile errors), then code. Spec and design updated.
- Production lines now about 567 (with docs and blanks) vs 390 before: over 500.

### G-a review fixes (feat/coding-tools-ga, round 2)
- Split: `tools/git/parse.rs` now holds the status/log parse and render (pure move, commit "refactor(tools): move git parse and render into a submodule"). Non-comment lines: `git.rs` 291 + `git/parse.rs` 203, each under 400.
- Identity (user decision 2026-10-27): the repo's effective identity (`git config user.name/user.email` in the repo, global in scope); G-b preflight reads it outside the jail and passes it to `Identity::new`. Replaces "global only" in design and spec.
- Log (user decision 2026-10-27): history simplification kept with the secret excludes (secret-only commits stay hidden); test pins no `--full-history`.
- Hooks: `GitDir::new(git_dir, common_dir)` pins `core.hooksPath=<common-dir>/hooks` (a linked worktree's git dir has no hooks); test for the linked-worktree case; `hooks_notice` says "lazarobox runs only the repo hooks folder".
- Hardening: status adds `--untracked-files=normal --renames`; `parse_log` drops records whose hash is not 40/64 hex or whose date is not strict ISO 8601 (a `\x1f` in the author shifts the fields); `check` refuses control and format characters in paths; temp-repo test for `add .` (stages all but secrets).
- G-b checklist recorded in design.md (cleared env, git >= 2.32, env vars never passed, truncated status stays truncated). Not implemented here.

### G-b: git runtime (feat/coding-tools-gb)
- Done: Gb.1, Gb.2, Gb.3. `runtime/tools/git.rs` (about 300 non-comment lines) + `git/tests.rs` (15 tests, 8 of them on a temp repo under `$HOME`, skipped without git or HOME).
- API: `Ctx{helper, git, home, root, scratch_parent, timeout}`; `open(&ctx, read) -> Repo{dir: GitDir, dirs, identity: Result<Identity, GitError>, configured_hooks}`; `run(&ctx, &repo, argv, read_verb, read) -> Ran{done, leftovers}`; `status_output(&Done) -> Output::{Text, Truncated, Failed}` (parse only `Text`); pure pieces `jail_spec`, `git_env`, `check_env`, `version_ok`, `pointer_dirs`.
- Design choices: no edit to `runner.rs`/`integrity.rs`/`scratch.rs`. Git is not a `Program` (that is the run_command allow-list), so it goes through `spawn_core` + `JailArgs` directly, and `toolchain_spec` is called with a placeholder `Program::Cargo` and `/usr/bin/git`; then caches are dropped (`rw` = the private TMPDIR + git dirs), net closed, toolchain rlimits.
- A git dir under the root gets no rule (R29: `rw` under the `rwx` root would be `WritableExec`; confirmed with the real helper). Dirs outside the root (linked worktree: `.git/worktrees/<n>` and the common dir) are `rw`, refused when broad (`broad_place`) or when `check_exec` fails.
- Preflight chicken-and-egg: in a linked worktree the git dir is outside the root, so even `rev-parse` cannot read it. `pointer_dirs` reads the worktree's `.git` file (`gitdir:`) and `commondir` and grants them `ro` for the preflight only; the real run then gets `rw` on what `rev-parse` reported. Verified by hand with the real `__jail-exec` (plain repo, linked worktree with ro and rw common dir).
- Identity and `core.hooksPath` are read by `git config -z --get-regexp` run OUTSIDE the jail (user decision: global config in scope), through `spawn_core` with a tiny env (PATH, HOME, XDG_CONFIG_HOME, `GIT_CONFIG_NOSYSTEM`, `GIT_TERMINAL_PROMPT=0`). It only reads values. An unset or invalid identity is `Err(GitError::Identity)` in `Repo.identity` (the commit is refused then).
- Env: scrub output filtered to PATH/HOME/USER/LANG/LC_ALL/TMPDIR/TERM (+ `XDG_CONFIG_HOME` when set) plus `core::git::env`. `check_env` fails closed if any `GIT_*` name beyond the fixed set is present or a fixed value is missing (covers `GIT_EXTERNAL_DIFF`, `GIT_PAGER`, `GIT_ASKPASS`, `GIT_SSH`, `GIT_CONFIG_COUNT/KEY_*/VALUE_*`, `GIT_CONFIG_PARAMETERS`, `GIT_DIR`, `GIT_WORK_TREE`).
- Version: `git --version` in the jail, must be >= 2.32 (`Fault::Version`).
- Gb.2 test has a control: a plain git run first proves the planted `core.fsmonitor`/`diff.external`/`core.pager` do create the marker; the hardened run through `run()` does not. The unit tests use a stand-in helper (no Landlock), so the real-jail evidence is the manual run above.
- Limits for G-c/G-d: (1) hooks live in `<common>/hooks`; for a worktree whose common dir is outside the root that dir is `rw` not `rwx`, so a hook cannot be executed in the jail (git ignores a non-executable hook). (2) A commit's hooks run in the jail and can write `.git/hooks` and `.git/config` (rw): the X-c3 integrity compare is NOT wired around git runs; G-c/X-d1 should decide. (3) tests need a root outside `/tmp` (broad place), so they use `tempdir_in($HOME)`.

### G-b review fixes (feat/coding-tools-gb)
- 1 (HIGH) planted `.git` file: `pointer_dirs` now returns `Result<Vec<PathBuf>, Fault>`. A git dir (or common dir) outside the root is accepted only as a genuine linked worktree: directly under `<common>/worktrees/`, `commondir` resolves to the common dir, and the `gitdir` back-link canonicalizes to `<root>/.git`; otherwise `Fault::Dir`. A non-worktree `.git` pointing outside the root (`--separate-git-dir`) is refused by design. `open` also refuses a rev-parse answer outside the root and the vouched dirs.
- 2 (HIGH) integrity around git runs: `jailed` takes the X-c3 snapshot before and after (same `AfterOnDrop` guard as the runner, now `pub(crate)`), `Ran.changes` added, `Ctx.unreported` sink added. `integrity::snapshot_with_git_dirs(root, extra)` also watches `config`, `config.worktree`, `hooks/*` and modules of the rw git dirs outside the root (keyed by absolute path), each through a beneath-open. The read-only preflight calls are not watched.
- 3 (MEDIUM) pointer reads: `read_small` opens beneath (no link followed), checks type and size on the opened file (regular, <= 4 KiB), `take()`s; `open` runs `pointer_dirs` in `spawn_blocking`.
- 4 (MEDIUM) unjailed `git config`: exit other than 0/1, timeout or truncated capture is `Fault::Preflight`; PATH goes through `scrub` (`safe_path`).
- 5 (LOW) temp-repo tests print a skip line; real-helper Landlock test `a_git_commit_in_the_real_jail_cannot_write_outside_its_grants` in `tests/jail.rs` (commit works, hook writes inside the root, a write outside the grants is denied).
- 6 (LOW) `Fault::Spec(PathBuf)` names the git path; comment on the `Program::Cargo` placeholder: git outside the toolchain's system rx places is refused by design.
- API changes: `pointer_dirs -> Result<Vec<PathBuf>, Fault>`; `Ctx.unreported: Unreported`; `Ran.changes: Vec<Change>`; `Fault::Spec(PathBuf)`; `integrity::snapshot_with_git_dirs`; `runner::{AfterOnDrop, watched}` now `pub(crate)` (`watched` takes extra dirs).
- TDD: tests first (RED = compile errors on the new API), then code; audit test forced `open_beneath` instead of `OpenOptions` for the pointer reads.
## Batch: slice X-d1 (host `run_command`, PR 26, branch feat/coding-tools-xd1, base feat/coding-tools-ga2)

| Task | Status | Commit |
|------|--------|--------|
| Xd1 core: `Call::RunCommand`, `run_command` ToolDef (Exec family), `ApprovalAsk::Exec`, minimal app prompt arm | done | feat(tools): offer run_command in the Exec family and always ask for it |
| Xd1.1 refusals before the ask (no jail, root policy, cwd, allow-list with `package.json` scripts, staged batch, busy chat) + Xd1.2 `RunGate` | done | feat(tools): refuse a command before it asks and gate one run per chat |
| Xd1.3 run after `y`: gate, job, `run_jailed`, `RunOutcome`, changes note | done | feat(tools): run an approved command in the sandbox and report what it changed |

Seam: all logic lives in the new `runtime/tools/host/{exec,gate}.rs` (children of `host`). `host.rs` edits (REBASE POINT against agent-orchestrator S1c-2, 18 lines): field `exec: exec::Exec` + its init in `ToolHost::new`; one arm in the pre-ask `match (valid, locality)` (`Call::RunCommand` -> `exec::plan`, refusal sets `checked = Err`, success asks with `ask_for`); one arm in the decision match (`(Decision::Sent, Call::RunCommand)` -> `exec::run`); `mod exec; mod gate;` and the test modules. `files::gate` also refuses an Exec call when the family is off (unknown tool). No seam avoids host.rs: the refusals need an async step before the ask, and the run is a new decision arm.

Design points:
- `Call::RunCommand { args: Value }` keeps the model's object (shape already checked by `RUN_ARGS`); the allow-list needs `package.json` so `exec::inspect` calls `parse_run(args, &scripts)` before the ask and again after `y`. Scripts come from the `package.json` of the resolved cwd (plain file, <= 256 KiB, not a link).
- Refusal order: family, jail (Landlock only), root policy (origin canonical, not HOME, `judge_real`), cwd (`plain_relative` + canonical inside the root), allow-list, text that would not show as it is (`clean(arg) != arg`), staged batch (`Staging` = "propose them first", `Proposed` = waiting for review), busy chat. All before any prompt.
- `ask_for(RunCommand)` always returns `ApprovalAsk::Exec{program,args,cwd}` (any locality, any mode; the app never routes it). The host refuses text the prompt could not show exactly, so `y` approves what runs.
- `RunGate`: `claimed: HashSet<StageKey>` + `Semaphore(2)`, 60 s wait cap, `Permit` frees both on drop (also a call cancelled while it waits). Taken after `y`, before spawn. `is_busy` is also checked before the ask.
- Result: `RunOutcome::render` (facts line, then the output wrapped as data); non-zero exit is an ok result; exit 125 plus the helper marker is an error ("the Linux sandbox could not be applied"). `changes_note` appends the watched-git-place changes (names inside the data wrapper, "not reverted"); an unchecked change adds "could not verify git hooks and config after this run". It is the ONE function that reads an integrity `Kind` (`Added|Changed|Removed` explicit, `_` = unchecked), so the `Crowded` -> `Unchecked` rename needs no edit. `Exec.sink(key)` is the per-chat `Unreported`; every result drains it. X-d2 can drain it for a notice.
- Private temp folder: `$XDG_RUNTIME_DIR/lazarobox-run`, else `<data>/lazarobox/run` (`Scratch::unique`, removed after the run).
- Program: first executable on the scrubbed PATH (pytest: `<root>/.venv/bin/pytest` first), canonical, passed as `Layout.program`; arg0 comes from the runner.

TDD evidence: core tests RED = `E0599 no variant RunCommand / Exec, no method is_exec` (4 errors); gate tests RED = `E0583 file not found for module gate`; host tests RED = `E0609 no field helper/read on Exec` (3 errors). The app prompt test and the host refusal tests passed on first run (written before the code, compile-RED). Real jail smoke (throwaway test, removed): `go test` through `target/debug/lazarobox-shell __jail-exec` started in Landlock and was refused by the jail on a GOCACHE outside the grants, as it should.

Known gaps / notes:
- The unchecked-kind case has no test here (the variant is `Crowded` on this branch and `Unchecked` after the rebase): add one `changes_note` test after the rebase.
- PATH is the process PATH, not the login-shell extended one the CLI engines use: a GUI launch may not find `~/.cargo/bin`. X-d2 or a follow-up can pass `CliEnv.path`.
- macOS: `jailed()` accepts only the Landlock backend.
- Tests that need a project outside `/tmp` (the toolchain spec refuses scratch folders) build their world under `target/`.

### X-d1 review fixes (feat/coding-tools-xd1)

| # | Finding | Fix |
|---|---------|-----|
| 1 | HIGH: a program printing the marker and exiting 125 returned before `ran.changes` was read | The helper reports "jail not enforced" on a status pipe (descriptor 3, number in `LAZAROBOX_JAIL_STATUS_FD`), closed by the `exec` of the program, so the program cannot write it. `Ran.not_enforced` carries it. `go` reads `ran.changes` and drains the sink FIRST and `failed()` appends the changes note on every error path. Exit 125 plus a forged marker is now a normal result. Descriptor plumbing lives in `runtime/proc.rs` (the audit forbids `libc::fcntl/dup2` in tool files). |
| 2 | MEDIUM: `.venv/bin/pytest -> /bin/sh` | A venv candidate counts only when its real path starts with the root; otherwise the search falls through to the PATH. |
| 3 | MEDIUM: npm/yarn/pnpm script bodies | `npm run <x>` is checked against the declared scripts. `ApprovalAsk::Exec` gained `script: Option<String>` (sanitized `name: body` lines, 200 chars each, with `pre`/`post`). The host keeps the whole `scripts` object at ask time (not a hash: nothing to collide) and refuses after `y` with "package.json changed after the question; ask again". |
| 4 | MEDIUM: output to a cloud model | `exec::plan` refuses `Locality::Cloud` ("commands run only with a local model for now"). Marked `REMOVED BY X-d2`: delete the check and the `CLOUD` const. |
| 5 | LOW-MEDIUM: prompt rendering | Args that are empty or have whitespace, quotes, `\`, `$` or a backtick show shell-quoted; args with control characters are refused; the folder moved from the clipped head row to a command row (`in <cwd>`), so `fits` checks it. Script lines show as `script ...` rows. |
| 6 | Test for `Kind::Unchecked` in `changes_note` | Added (`an_unchecked_place_says_the_project_may_be_tainted`). |

TDD evidence: RED seen for 1 (runner test `a_helper_that_reports_on_the_status_channel_is_not_enforced`, jail integration tests, 3 host tests), 2 (3 venv tests), 3 (2 host tests, 1 allow test), 5 (5 app tests, 1 host test). 4, 6 and the `script_line` test were written together with the code (they passed at first run).

API notes for X-d2: `Plan` has `pin: Option<Pin>`; `exec::run` takes the ask-time `Option<Pin>`; `ApprovalAsk::Exec` has `script`; `ask_for` leaves it `None` and the host fills it. The prompt command is now `cmd\nin cwd[\nscript ...]` and the target is the fixed "run a command in the sandbox": the X-d2 notice goes in `why`, not the head. Remove the cloud check in `exec::plan`. `runner::spawn_with` is private; `Ran` gained `not_enforced`; `spawn_core`/`Run`/`Done` unchanged (git.rs untouched).

## Batch: slice X-d2 (Exec ask and notice, branch feat/coding-tools-xd2, base feat/coding-tools-xd1)

| Task | Status | Commit |
|------|--------|--------|
| Audit 3: a run that fails after it started reports its git changes (`after_failure` takes the after-snapshot before returning the error) | done | fix(tools): report git changes of a run that failed after it started |
| Audit 1+2: install/ci show the lifecycle scripts (pinned like other scripts); npm/yarn/pnpm `test`/`run` refused without a `package.json` in the cwd | done | fix(tools): show install scripts and run npm scripts only beside package.json |
| Xd2.1 notice, timeout, tier-B, cloud flag, earlier-run note; cloud refusal removed | done | feat(tools): ask for every command with the env, network and output notice |
| Xd2.2 Exec menu row reason | done (no-jail); global-wait row DEFERRED | feat(menu): say on the Exec row that commands need the Linux sandbox |

Decisions / deviations:
- The notice is NOT in `ApprovalPrompt.why`: `why` is advice that is dropped when the pane is short, and the spec says the notice MUST be visible. `ApprovalPrompt` gained `notice: Option<String>`, drawn as `Part::Notice` rows after the command and counted by `fits` (own budget on top of `MAX_COMMAND_ROWS`), so a pane too small for it does not let `y` through.
- `ApprovalAsk::Exec` gained `timeout_secs` (host fills the clamped value), `cloud` (`ask_for` sets it from the locality) and `earlier` (host fills from `Unreported::peek`, the sink stays intact for the model). `exec::fill` replaces the inline script fill in `host.rs` (net fewer lines there).
- Tier-B: static per-program line of the project files the program runs (`allow::runs_files`), plus the install line (`allow::installs`). It does not look at which files the chat wrote.
- Audit 2 chose the refusal: "run it from the folder that has package.json". The old yarn test now expects that text.
- `JailSupport::runs_commands` is the single Landlock-only check (host `jailed()` and the menu share it). `MenuState::with_sandbox` default is "fine" so existing menu snapshots are untouched; the app passes its probe.
- DEFERRED: the live "waiting for a slot" state of the global cap. The wait happens in `RunGate::acquire` after `y`; showing it needs a new stream event (the transcript row has no update channel). The refusal after 60 s already reads "too many commands are running; try again later".
- README has no Exec/coding-tools section on this branch, so it was not touched; CHANGELOG `[Unreleased]` was written.
- Optional LOW done: `display_word` quotes `; | & < > ( ) * ? # ~ { } !`.

TDD evidence: audit 3 RED = `E0425 no function after_failure`; audit 1/2 RED = 2 host tests failing (install script not shown, no refusal outside the package folder); Xd2.1 RED = compile errors on the new `Exec` fields, `exec_notice`, `Part::Notice`, `ApprovalPrompt.notice`, `peek`; Xd2.2 RED = `E0599 no method with_sandbox`.

### X-d2 review fixes (feat/coding-tools-xd2)

| # | Finding | Fix |
|---|---------|-----|
| 1 | MEDIUM: `npm/yarn/pnpm install\|ci` outside a `package.json` folder ran unseen parent scripts | `runs_scripts` is now any package-manager call: with no `package.json` in the cwd it is refused ("run it from the folder that has package.json"). Test loops test, run, install and ci. |
| 2 | MEDIUM: unbounded paths in `earlier_note` could push the notice past the row cap and disable `y` for the chat | Each path is cut at 60 columns, the line at 240: "N places under .git were touched by a cancelled run (not reverted): a, b, c, …". `exec_notice` no longer prefixes it. Tests: very long paths stay within 240 columns and the prompt fits at 80 and 100 columns. |
| 3 | LOW: `--ignore-scripts` | `allow::skips_scripts` (flag present and not turned off by `=...` or `--no-ignore...`): the notice says "install scripts are skipped" and the pin shows no bodies; otherwise "and, depending on the package manager, its dependencies" plus "`--ignore-scripts` skips them". |
| 4 | LOW: `pin()` hooks | Install also shows `preprepare`/`postprepare` (npm) and `prepublish` (yarn). |
| 5 | LOW: tier-B | `.npmrc` for npm/pnpm, `.yarnrc.yml` for yarn (`allow::runs_files`). |
| 6 | LOW: `wanted` cap by width | When the text would fit the row cap at a wide pane, the marker says "widen the pane" (`TOO_WIDE_TEXT`); otherwise the old marker. Still fail-closed: `fits` unchanged. |
| 7 | LOW: `display_word` | Also quotes `[`, `]`, `^`. |
| 8 | Tests | `a_run_that_cannot_start_is_an_error_that_still_reports_the_sink` (missing helper -> `run_jailed` Err arm; fails if the arm propagates the error); `route` of an Exec ask in every mode and origin; app test that every approval mode prompts with the notice. |

TDD evidence: RED = `E0603 earlier_note is private` (compile), then mutation checks on the finished code: restoring the `test|run` filter in `runs_scripts` fails `test_and_run_need_the_folder_that_has_package_json`; replacing the Err arm by `return Err(COULD_NOT)` fails the new Err-arm test. Pure additions on the notice/marker side were written with the code.

## Batch: slice G-c (host git read verbs + GitDiff ask, PR 28, branch feat/coding-tools-gc, base main)

| Task | Status | Commit |
|------|--------|--------|
| Gc.2 core: `Call::{GitStatus,GitDiff,GitLog}`, three `ToolDef`s in the `Git` family (Access::Read), `ApprovalAsk::GitDiff`, `ask_for` (diff asks on Cloud only), app prompt arm | done | feat(tools): offer git_status, git_diff and git_log in the Git family and ask for a cloud diff |
| Gc.1 host: refusals before the ask (family, jail, root, staged batch), run after `y` through `git::open` + `git::run`, output shaped and wrapped | done | feat(tools): run git status, diff and log in the sandbox and gate them like other reads |

Seam: all logic lives in the new `runtime/tools/host/git.rs` (child of `host`). `host.rs` edits (REBASE POINT against agent-orchestrator S1c-2, 10 lines): one pre-ask arm `(call, locality) if call.is_git()` (`git::plan`, a refusal sets `checked = Err`, success asks with `ask_for`), one decision arm `(Decision::Sent, call) if call.is_git()` -> `git::run`, `mod git;` and the test module. `files::gate` gained the Git family check (one condition). `exec.rs`: `jailed`, `staged`, `scratch_parent` became `pub(super)` and the root policy was extracted to `root_of` (shared by `inspect` and git).

Design points:
- Spec vs design conflict: the spec text says every read verb needs `y` on a Cloud engine, while the design table and the task say only `git_diff` does (status and log return names and subjects). The task and design were followed (`git_status`/`git_log` never ask). If the user wants the spec text, it is one `ask_for` arm each plus a prompt variant.
- `parse_built_in` takes the first-argument text only when that argument is required, so tools whose first arg is optional (`git_diff`, `git_log`) parse with all args absent.
- `git_log` `max_count` is an `Int` clamped to `1..=50` by the table; `git_diff` `staged` is a `Bool`. `path` is checked by `diff_argv`/`log_argv` (secret, `.git`, escape) after the question, so a refused path costs a prompt on Cloud. Candidate follow-up: refuse in `plan` before asking.
- Result: status and log go through `status_output` (cut = error "too large to list", never half-parsed), `render_*`, `redact_tokens`, then `label::wrap`; a diff uses `Capture::text` (clean, cut, redacted) and `(no changes)` when empty. A non-zero exit is an error "git failed". Changes to watched git places during the run (a filter driver) are appended via `exec::changes_note`.
- The read verbs take no `RunGate` claim: they are short, read-only and may overlap a command of the same chat.
- Fault texts are fixed (`NO_GIT`, `NOT_A_REPO`, `TOO_OLD`, `REPO_REFUSED`, `COULD_NOT`); a git outside a system place says so.
- Tests need a git in a system place (the jail refuses any other) and skip with a stderr line otherwise; the staged-batch test is `cfg(linux)` like the exec one. The jail helper is a stand-in script.

TDD evidence: core RED = `E0599` on `Call::GitStatus/GitDiff/GitLog`, `ApprovalAsk::GitDiff`, `is_git` (16 errors); after GREEN one test failed with `MissingArgument` (optional first arg) and drove the `parse_built_in` fix. Host RED = 7 of 9 tests failing with the verbs unknown to the host (the other two passed trivially on the old error path); GREEN = all 10. The app prompt test and the refused-path test passed at first run (written with/after the arm).

Known gaps: no `git_status`/`git_log` ask on Cloud (see above); no concurrency gate for reads; `git` is looked up on the scrubbed process PATH (a GUI launch may lack Homebrew paths, which the jail refuses anyway).

## G-c review fixes (judgment day)
- Spec and design amended (user decision 2026-10-28): on Cloud only `git_diff` asks; `git_status` and `git_log` run free. Code unchanged for this point.
- `git::plan` now takes the call and runs the pure `check` (now `pub(crate)`) on the `path` of `git_diff`/`git_log`, so a refused path (secret, `.git`, `..`, empty) opens no prompt. `host.rs` edit: one argument.
- `find_git` keeps only git binaries whose real path is under `SYSTEM_RX`; a user-installed git first on PATH no longer hides a system one.
- New `Fault::Root` (from `Refusal::Root`/`Never` via `spec_fault`) maps to `REPO_REFUSED`; only `Fault::Spec` maps to `NO_GIT`.
- Status and log use the full `redact` (same as the diff), not `redact_tokens`. Test: `password=` and `api_key=` in a commit subject.
- Test hygiene: `sh()` drops `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, sets `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1` and an explicit identity. The `cfg(target_os = "linux")` on the staged-batch test was dropped (module is unix-only and the test uses no Linux API; it passes).
- TDD: the redaction test was observed RED against `redact_tokens` before the fix.

## Batch: slice G-d (host git add and commit + CommitReview, PR 29, branch feat/coding-tools-gd, base feat/coding-tools-gc)

| Task | Status | Commit |
|------|--------|--------|
| Gd.1 `git_add`: `Call::GitAdd`, `GitWrite` tool defs, one-line `ApprovalAsk::GitAdd`, host plan (path refusals before the ask, dry run count) and run | done | feat(tools): add and commit through git_add and git_commit with a reviewed commit |
| Gd.2 `git_commit`: `ApprovalAsk::Commit(Arc<CommitReview>)`, review rows, index secret check, staged diff pin, unsigned, hooks in the jail | done | same commit |
| Gd.3 end-to-end in a temp repo: commit refused on a staged `.env` (also as a rename away from a tracked one) | done | same commit |

Seam: all logic is in `runtime/tools/host/git.rs` (`plan` now returns `Option<ApprovalAsk>`; `run` takes the reviewed ask), `core/ai/chat/review/commit.rs` (new child of `review`, so it builds `ReviewDoc` rows) and the pure argv in `core/ai/tools/git.rs`. `host.rs` edits (REBASE POINT against agent-orchestrator S1c-2, 11 lines): `let mut reviewed`, the git arm now `call.uses_git()` and stores the planned ask, the decision arm passes `reviewed.take()`. `files::gate` gained the `GitWrite` condition (one line). `Call::uses_git()` and `is_git_write()` are new.

Decisions / deviations (needing the user's eye):
- The add overlay shows the paths and the FILE COUNT of a dry run, not a diffstat (+/-). Reason: untracked files have no diff; the count is the exact list git would stage after the secret excludes. Flagged for the user; a numstat sum is an easy follow-up.
- No review-view fallback for `git_add`: a prompt that does not fit blocks `y` (existing `fits` rule) and an add whose paths do not show whole in 400 characters is refused before the prompt ("add fewer").
- `git_commit` is refused before the ask on: empty/oversized/control-char message, unusable repo identity, nothing staged, a secret path in the INDEX (`diff --cached --name-only -z --no-renames`, so a rename away from a tracked `.env` counts), a staged diff the capture cut. A staged diff of the secret-excluded kind is therefore never shown or hidden: the commit does not happen.
- The review diff passes `--ignore-submodules=none` (superseded in the G-d review fixes below: it was only omitting the flag, which `.gitmodules` `ignore = all` defeated).
- After `y` the host rebuilds the staged diff and compares it to the reviewed bytes (`CommitReview::same_staged`); a change gives "the staged changes changed after the question; ask again".
- A workers' turn has no `stage_key`, so `git_add`/`git_commit` are refused as unknown tools there (the orchestrator chain still has to make worker scopes keep `stage_key = None`).
- The rejection line says "commit rejected: N files, +a -b".
- The `run` step no longer re-checks the `path` of diff/log itself: `diff_argv`/`log_argv` check it (same refusal).
- The hook-in-jail scenario is proven by construction (commit goes through `runtime::run`, the same jailed path as the read verbs, with rw only on the git dirs); the host tests use the stand-in helper, so they prove that hooks run from the pinned folder, that a failing hook blocks the commit, that its output is wrapped data and that a repo `core.hooksPath` is not run.

TDD evidence: Gd.1 RED seen as 6 failing host tests (unknown tools / no ask) after the `Call` and registry types existed, and a failing parse test (`MissingArgument` on a list-first tool) that drove the `parse_built_in` fix. The pure argv helpers and `CommitReview` were RED as compile errors only. For the commit host path the code was written before its tests, so mutation checks were run on the finished code: removing the index secret check fails `a_commit_is_refused_when_the_index_holds_a_secret_path`; replacing the staged-diff comparison fails `a_commit_is_refused_when_the_index_changed_after_the_question`; disabling the cut-diff refusal fails `a_staged_diff_too_large_to_review_is_refused_before_the_prompt`; flipping `commit.gpgsign=false` fails the unsigned and `core.hooksPath` tests; dropping `Commit` from the generic-approve guard in `app/run.rs` fails `a_generic_approve_never_answers_a_commit`.

Size: about 560 production lines added (adds+deletes about 598), over the 450 estimate; tests are about 700 more.

### G-d review fixes (2026-10-28, both judges confirmed)

- Review blind spots: `review_diff_argv` adds `--text` and `--ignore-submodules=none`; `index_names_argv` adds `--ignore-submodules=none`. The weak argv-absence test was replaced by two real-git host tests (`.gitattributes` `-diff`; `.gitmodules` `ignore = all` with a staged gitlink bump). Both were RED (binary rows; "nothing is staged") before the fix.
- Post-commit verification (USER DECISION 2026-10-28: warn, never undo): `write-tree` before the commit, `HEAD^{tree}` after, `diff-tree` between the two for the paths, `drift_notice` (pure, secret names masked, capped at 10 names) for the text. The outcome is an error (no `ToolStatus::Warning` exists) whose summary reaches the row. Nothing is ever reset. Tests: a pre-commit hook that `git add -f .env` (secret reported, HEAD kept), a hook that restages another file (named, no secret claim), a harmless hook (plain success). Only the tree is compared, not the message (a `commit-msg` hook edit is not detected: the whitespace cleanup would have to be re-implemented).
- `CommitReview::new` takes the running hooks and shows "hooks that will run: ..." (warning) or "no hooks will run" (note); the 60x20 snapshot changed by that line.
- An add path with a quote is refused before the prompt (`GitError::Quote`, also `add_line` returns `None`).
- `git commit` runs with a 120 s timeout (`COMMIT_TIMEOUT`); read verbs keep 30 s.
- `host.rs` untouched.
