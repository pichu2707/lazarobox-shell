# Design: coding-tools (search, write, run and git tools)

Inputs: `proposal.md` (engram #3330, with its resolved questions), user decisions #3326, #3327, #3328 and #3331, `exploration.md` (#3325), and the agent-orchestrator design (#3310). Base: origin/main 8aeddf8. Line refs were re-checked on this branch. Linux only. Toggle names follow the parallel `specs/configuration` delta.

## Technical Approach

There are three layers, following the repo's existing core/runtime split:

- `core/ai/tools/*` holds pure rules: the registry, arg parsing, glob, edit, diff, the stage state machine, the allow-list and git argv.
- `runtime/tools/*` holds IO: openat2 writes, the stage store, apply, the runner and git.
- `app` + `ui` hold the review view.

Writes never touch disk while staged. One tool, `propose_batch`, turns the staged set into a single coverage-gated review. Runs and git go through one runner primitive, which spawns `lazarobox __jail-exec` with a new toolchain profile. Every family sits behind a toggle that is off by default. With all toggles off, the registry, the parser and the executors behave exactly as in 0.18.0.

## Architecture Decisions

| # | Topic | Options and tradeoff | Decision |
|---|---|---|---|
| D1 | Registry shape (P0) | Keep the tuples and add a side table: two sources of truth. A `ToolDef` table drives the schema and the parse. | **`ToolDef{name, description, access, family, args: &[ArgDef]}`** in a new `core/ai/tools/registry.rs`. `ArgDef{name, kind, required, gate: Option<Family>}`. An arg with a `gate` is offered and accepted only when that family is on. This is how `read_file` gains `start_line`/`max_lines` under Search while its schema stays byte-identical with Search off. `ToolSpec`, `registry()`, `parse_call()`, `parse_call_with()`, `Call::name()` and `Call::argument()` keep their signatures. |
| D2 | Parse error order | A generic validator could reorder errors. | Fixed order: unknown → not an object → per ArgDef in order (missing, wrong type) → extra key → too long. This is the 0.18.0 order for the one-arg tools. Ints are **clamped** to their range and never refused (`timeout_secs` 900 becomes 600, reported in the result). No new `ToolError` variant, so mcp.rs `refusal_text` is untouched. |
| D3 | Family gate | Filter only the offered list: but `parse_call_with` accepts any built-in name today (mcp.rs:352, stream.rs:331), so the model could call an unoffered tool. | **`FamilySet` (a Copy bitset) is the single gate.** `registry_with(external, set)` and `parse_call_in(name, args, external, set)` gate on it. A built-in outside the set is `UnknownTool`, with the same text as an unknown name. `registry()` and `parse_call_with()` mean `FamilySet::LEGACY` = {Read, Pane}. `Family` gains `GitWrite` as its own variant, so the `git`/`git_write` toggles stay one bitset. External tools stay outside `Family`; the caller passes `&[]` to drop them. host.rs re-checks as defense in depth once the coding-tools slices land there. |
| D4 | Where staged batches live | (a) app state: but reads in the runtime need them. (b) per `TurnScope`: lost when the turn is cancelled. (c) **a runtime `Stages` store owned by `ToolHost`, keyed by `StageKey`**. | **(c)** `StageKey::Pane(PaneId)` now. The S7 work adds `Worker{..}`; the enum is designed to grow. One batch per key. Memory only; it never survives a restart. |
| D5 | Batch lifecycle | Deny returns the batch to staging, or deny discards it. | `Staging → Proposed(gen) → Applied \| Discarded`. While a batch is Proposed, staging is refused. A user `n` or Esc discards it (spec). A CLI timeout, or the turn cancelled while the ask is pending, leaves it **Proposed-orphan**. An unproposed batch whose turn ended is a **Staging-orphan**. The review and apply carry `BatchId{key, gen}`, and apply refuses a stale gen. |
| D6 | Batch caps | Check only at propose: the model then has no way to split. Check at stage time and at propose. | Both. A stage call that would pass 50 entries or 1 MiB of new content is refused with "batch is full: propose it, then stage the rest". `propose_batch` re-checks, as the spec requires. Base snapshots (needed for the diff and the rollback) are held in addition, capped by the per-file limits (worst case about 51 MiB per pane). |
| D7 | Reads of staged content | Only `read_file`, or every read tool. | `read_file`, `glob`, `grep` **and `list_dir`** read through an immutable `StagedView` snapshot passed in `ToolEnv`. Staged paths are marked `(pending, not on disk)`. Without `list_dir`, a model that just created `src/` would be told it is empty. |
| D8 | Diff | `similar` (pure Rust, no deps, Myers with a deadline). A hand-written Myers (about 200 lines plus tests, our own bugs). `imara-diff` (fast, but a less ergonomic hunk API). `git diff --no-index` (needs git and runs git config). | **`similar` 2.x**, `TextDiff::configure().algorithm(Myers).deadline(500 ms)`, 3 lines of context. A create needs no diff (all lines are added). |
| D9 | Review view and coverage | A frontier of the furthest row seen breaks on resize and jumps. A per-logical-line `seen` bitset. | The pure `ReviewDoc` holds headers, warnings and hunk lines. `layout(doc, width)` is the ONE wrap function, shared by the renderer and the gate. A logical line counts as seen when all its wrapped rows were on screen. **`y` is enabled when every line is seen AND 800 ms have passed since the view opened.** Other asks keep `ARM_DELAY` at 400 ms. `n` or Esc rejects. Invisible characters in existing context lines render as `<U+XXXX>`. |
| D10 | Apply atomicity | Write in place, which cannot be rolled back. Temp file plus rename per file, with a journal. | Preflight every target: open the parent with openat2 `RESOLVE_BENEATH\|NO_SYMLINKS`; the target must be a regular file with nlink 1, or absent for a create; the bytes are compared with the snapshot (drift refuses the whole batch). Then, in order (dirs by depth, then files): `mkdirat`, or an `O_EXCL` temp file + fsync + `renameat2(NOREPLACE)` for a create, or `renameat` for a replace (keeping the base mode). Each step is journaled. On failure, undo in reverse: unlink the created file, restore the base via temp+rename, rmdir an empty created dir. If an undo fails, the result is `Partial{changed paths}`. Apply is behind a `WriteOps` trait so tests can inject a fault at step k. |
| D11 | Content rules | Clean the content, or refuse it. | Refuse, never clean (`core/ai/tools/content.rs`). Refused: C0 controls except `\t` and `\n`, DEL, C1, `\r`, and the fixed list U+00AD, U+061C, U+180E, U+200B–200F, U+202A–202E, U+2060–2064, U+2066–2069, U+FEFF, U+FFF9–FFFB and U+E0000–E007F. ZWJ is refused too, per the proposal's "zero-width". An existing file must be UTF-8 with no CR or NUL. No trailing newline is added; the review shows `no newline at end of file`. |
| D12 | Write deny (`paths::resolve_write`) | | The root is the origin. It is refused when it is HOME, an ancestor of HOME, a system dir, missing, or **outside HOME** (the same rule as reads). The target must be beneath the root. Refused: any `.git` or `node_modules` component, and SECRET_DIRS/SECRET_FILES. One refusal text for every case: `path not available`. Tier-B warnings: `Makefile`, `build.rs`, `Cargo.toml`, `.cargo/config*`, `package.json`, `pyproject.toml`, `setup.py`, `conftest.py`, `go.mod`, `*.sh` and `.github/workflows/*`. Reads: a file named `config` or `config.worktree` under a `.git` dir is refused.Known gap, accepted for now: bare or relocated git dirs (for example `repo.git/config`, or `.bare/` reached through a `gitdir:` file) are not covered by the `.git` component rule. |
| D13 | Overwrite and edit | | `write_file` on an existing file is accepted only when the file is ≤ 64 KiB (the read cap) and `redact(clean(text)) == text`. Otherwise: "use edit_file". `edit_file`: `old` must be non-empty and match exactly once in the staged-or-disk text; each field ≤ 16 KiB; the file ≤ 1 MiB. |
| D14 | Runner shape | R1 unjailed. R3 in the pane. **R2: a host child through `__jail-exec`.** | R2, built on `std::io::pipe` (Rust 1.88 ≥ 1.87). stdout and stderr share one pipe so the order is kept. stdin is `/dev/null`. The child gets `process_group(0)` + `GroupKill`. Timeout and cancel kill the group; a normal exit also uses `kill_and_reap`, then drains the pipe for 500 ms. |
| D15 | Toolchain jail | Reuse the CLI profile: it lacks `/usr/bin` and write+exec. A new profile. | A pure `core/ai/toolchain.rs` builds a `JailSpec` (see the next section). `JailSpec` gains `rwx`, and the helper gains `--rwx`, `--net-bind open` and `--rlimits toolchain`. Invariant R29 is amended: Execute appears only in `rx`, or in the single `rwx` project root of a toolchain run. |
| D16 | TCP bind | Keep the CLI rule (no listen), which breaks tests that bind `127.0.0.1:0`. | The toolchain profile does **not** handle BindTcp. Tests are approved code, and outbound traffic is open anyway (a user decision). |
| D17 | Concurrency | | `RunGate` in `ToolHost`: one run per `StageKey` (a busy key is refused at once: "a command is already running in this chat") and a global `Semaphore(2)`. The global wait is shown on the row, capped at 60 s, then refused. Both are taken after `y` and before spawn. |
| D18 | Runs with no jail | Hide the tool, or offer it and refuse it. | Offered when toggled on. A call is refused with "commands need the Linux sandbox (Landlock ABI 3+)", so the model stops and tells the user. The menu row shows the same short reason. |
| D19 | Git implementation | gix (large tree). git2 (C build, no hook parity). **The git binary through the runner.** | The `git` binary through the runner, using a separate git `JailSpec` and fixed argv templates (the Git section below). |
| D20 | Worker batches | | A worker stages under its own key. Its `propose_batch` ask goes through the S3 FIFO, labelled `worker n/m kind engine>model`, and opens the same review view. A worker batch left unproposed when the worker ends is discarded, and `WorkerResult` says so: workers never leave orphans. Workers never commit. |
| D21 | Toggle rollout | Proposal M5: toggles last, so M1–M4 would ship without one. | **The toggle plumbing lands right after P0 (slice T).** Each milestone ships dark. M5 keeps the README and threat-model work. |
| D22 | Write primitives (W-b) | Trust `resolve_write`, or re-open at write time. | **Re-open at write time**, in `runtime/tools/write.rs` (not `open.rs`: `audit.rs` bans every write API from the read-side files, and a test keeps `fs.rs`, `open.rs` and `search.rs` from naming `write`): `open_target`, `make_dir`, `create_file`, `replace_file` each open the parent with `open_checked` (openat2 `RESOLVE_BENEATH\|NO_SYMLINKS`, `fd_path == real`, `judge_real` again), then act relative to that descriptor. An existing target must be a regular file with `nlink == 1` opened with `O_NOFOLLOW` (a final link, a hard link, a FIFO or a folder is `Refused`) whose own fd path is judged again. The temp file is `.lbx-<32 hex from getrandom>.tmp` in the same folder (`O_EXCL`, retried on a clash), removed by a drop guard on every error path. Create ends in `renameat2(RENAME_NOREPLACE)`; replace in `renameat` after `fchmod` to the base mode (owner and group are not restored). Replace takes the bytes the user reviewed and compares them with the bytes read through the opened fd; a difference is `Drifted` and nothing is written. The preserved mode is masked with `0o777` (never setuid, setgid or sticky); owner, group, ACLs and xattrs are not preserved. Both calls check, after staging and before the rename, that the parent descriptor still resolves to the judged parent path (a moved folder is `Refused`). The open of an existing target goes through `open_existing_at`, whose flags are fixed (read only, `O_NOFOLLOW`), so `write.rs` never picks open flags. Replace renames over the name, so a hard link or a link swapped in later is replaced, not written through. A leftover temp (crash, `SIGKILL`, power loss) is hidden from `list_dir`, `glob`, `grep` and `find_dir` and refused as a read or write target by one shared predicate (`is_temp_name`, part of `is_excluded_name`); the random name means a leftover never blocks the next create. `Cargo.toml` sets no `panic = "abort"`, so the drop guard runs on a panic; it does not run on an abort or a kill. The name comes from `getrandom` with `GRND_NONBLOCK`, retried on EINTR; if the pool is not ready a time, pid and counter nonce stands in (`O_EXCL` guarantees no clobber). `audit.rs` scans every `.rs` file under `runtime/tools/` (test-only modules excepted): process and socket bans hold everywhere, write bans everywhere but `write.rs`, which has its own `libc` allow-list, and the `write` module may be named only in `write.rs`, `mod.rs` and `audit.rs`. The folder is synced best effort. A filesystem without `RENAME_NOREPLACE` makes a create fail with `Io`, never replace. Off Linux every primitive returns `Unsupported`. |
| D23 | Content scope and BOM | `check_new` on the whole post-edit text, or on the changed text only. | **Only the new text** (the `new` string of an edit, the whole `content` of a create or overwrite). Running it on the whole result would make a file that already holds an odd character impossible to edit; `check_existing` is what guards the old text (UTF-8, no NUL, no CR). The write primitives take bytes and never inspect them. BOM asymmetry, on purpose: `check_new` refuses a leading U+FEFF (an invisible character in text a model produced), `check_existing` accepts a BOM already on disk. An edit that leaves the BOM where it is passes; a model cannot add one. |
| D24 | Toolchain spec and macOS (X-c1) | Render `rwx` in SBPL now, or refuse it. | **Refuse on macOS.** `JailSpec.rwx` is a new class; `JailSpec::check_exec(os, homes)` is the amended R29 (`ExecFault`: more than one `rwx`, not canonical, `/`, or `Os::Macos`) and `sbpl::render` returns `SbplError::Rwx` for any `rwx` entry, so no profile exists for it and the macOS overlap check (`writable_executable`) never sees one. CLI specs never carry `rwx` (test over all three systems). `core/ai/toolchain.rs` is the pure builder (`toolchain_spec(&Layout, resolve)`), `toolchain/env.rs` the env scrub (`scrub(read, root, tmp)`). Root: refused when broad for any CLI (`jail::broad_place`: HOME or above, scratch, system, any credential folder). Program under the root is refused except `<root>/.venv/bin/pytest`; a program outside every known place (system, `$CARGO_HOME/bin` + `$RUSTUP_HOME`, nvm/volta/fnm node folder, `GOROOT` or the folder above `bin`, pytest venv with `pyvenv.cfg` or the one file) is `Refusal::Program`. Any granted `rwx`/`rx`/`rw` path equal to, under or over `~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.gitconfig`, `$CARGO_HOME/credentials*` is `Refusal::Never`. Landlock rendering of `rwx` and the helper flags belong to X-c2; until then `JailArgs::render` and `rules()` ignore `rwx` (fewer rights, never more). | **Review (R29 on Linux too):** `toolchain_spec` drops a cache (`rw`) that is equal to, under or over an execute path (`rx`, `rwx`) instead of refusing the run, because a cache is optional and the tool still runs without it (Go with `~/go/bin/go` and `~/go/pkg/mod`; an `rwx` root over a cache). Anything else writable next to an execute path (the private TMPDIR, `rw_files`) is `Refusal::WritableExec`, and `check_exec` re-checks it as `ExecFault::WritableExec` for the helper. The Docker rust layout (`CARGO_HOME=/usr/local/cargo`) is refused earlier, as `Refusal::Never("/usr")`: the cargo credential files would sit inside the system `rx`, which Landlock cannot carve out. `check_exec` also rejects a broad `rwx` root (`broad_place`: `/home`, `/usr`, `$HOME`, a credential folder). A `go` that the system `rx` already covers (`/usr/bin/go`) gets no `go_root`.

**Staged view notes (W-e3 review).** The marker words `pending, not on disk` are defanged to `pending; not on disk` in disk text and names ALWAYS, view or not (in `execute_over`, `grep_over` and `glob_over`), so a missing view can never make the marker forgeable; the only byte difference from before is for files that contain those literal words. The staged lookup resolves the asked path like a disk read (deepest existing ancestor canonicalized, no `..` folded by text), because staged keys are real paths.

## Toolchain jail profile (Linux)

| Class | Paths |
|---|---|
| rx | `/usr`, `/bin`, `/lib`, `/lib64`, `/usr/local`. Toolchain roots resolved from the program's canonical path: `$CARGO_HOME/bin` and `$RUSTUP_HOME`, the node root of nvm, volta or fnm (`package_root`), `GOROOT`. pytest: a PATH install outside the root, or `<root>/.venv/bin`. |
| rwx | the project root (origin), which holds `target/`, `node_modules/` and `.venv` |
| rw | a private TMPDIR per run (removed after the run). Caches that exist: `~/.cargo/registry`, `~/.cargo/git`, `~/.npm`, `~/.cache/{pip,yarn,go-build,pre-commit}`, `~/go/pkg/mod`, `~/.local/share/pnpm` |
| ro / ro_files / rw_files | `LINUX_RO`, `LINUX_RO_FILES`, the `ETC_LINKS` targets, the `DEV_RO_FILES`; `/dev/null` (rw) |
| never | `~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.cargo/credentials*`, `~/.gitconfig` (run profile), the rest of HOME. Landlock is an allow-list, so these are unreachable by default. |

Landlock ABI limits:
- 3 (Linux 6.2) is the minimum.
- Below 4 (6.7), TCP is not mediated at all. That is acceptable because the network stays on.
- 6 (6.12) adds the signal and abstract-socket scope, which `enforce` already applies.
- **No ABI up to 8 mediates `connect()` to existing pathname UNIX sockets**, for example the gpg-agent or D-Bus sockets under `/run/user/$UID`. This is an accepted, documented residual risk; a seccomp AF_UNIX filter is a later hardening option. Checked in X-c2: ABI 9 adds `LANDLOCK_ACCESS_FS_RESOLVE_UNIX` (landlock crate 0.4.7, `AccessFs::ResolveUnix`: "connect to a pathname UNIX domain socket"), which the helper already handles through `AccessFs::from_all` on an ABI 9 kernel, so there a socket needs a rule. The local kernel headers (6.19) do not define it, so on ABI <= 8 (this machine included) the only guard is that `$HOME` secrets and `SSH_AUTH_SOCK`/`DBUS_*` env are not passed (env scrub). Scoping applies only to abstract sockets (ABI 6).
- Below ABI 6, a child can also signal same-uid processes (DoS only).
- Daemonizing grandchildren (`setsid`) escape `killpg`. This is a documented limit.
- rlimits are set in the helper before `exec`: CORE 0, FSIZE 4 GiB, CPU 1200 s.
- The helper refuses an `rwx` root that is a broad place (`$HOME`, `/home`, `/usr`, `/tmp`, ...) or whose `canonicalize` differs from the given path (a symlinked root), and lowers an rlimit to `min(parent hard limit, value)` instead of failing with EPERM; a value the platform cannot hold is an error, never "no limit".
- Residual risk (accepted): the `rwx` root is readable and writable as a whole, so a `.env` or hooks inside it (`.git/hooks`) are reachable by the toolchain; see the X-c3 notes below for the integrity check.

Toolchain run notes (recorded in X-c1 for X-c3, not implemented):
- `go test` and `go run` build and start binaries, so they need an exec-capable `GOTMPDIR` under the `rwx` root (for example `<root>/.lbx-gotmp`, created before and removed after the run); the private TMPDIR is `rw` only and never executable.
- cargo writes `$CARGO_HOME/.package-cache` and `$CARGO_HOME/.global-cache`; they are not in the cache list, so check by hand that a run with a populated registry still works, and add them as `rw_files` if it does not.
- USER DECISION: before and after each `run_command`, X-c3 compares `.git/hooks/`, `.git/config` and `.gitmodules` in the root; on a change it warns with a diff and offers to revert. The program can write them (the root is `rwx`) and a hook runs later outside the jail.
- X-c3 DONE: `runner::run_jailed` creates `GOTMPDIR` as `<root>/.lbx-gotmp-<pid>-<n>` for Go only (removed after the run, also on drop) and sets `GOTMPDIR` after the scrub. The cargo check was run by hand in the real jail (populated registry, and an index update): `.package-cache` needs NO `rw_files` entry (cargo works without writing it). `.global-cache` is an SQLite file that also needs a journal next to it in `$CARGO_HOME`, which is not writable (granting it would mean writing `~/.cargo`, next to `bin/` and `config.toml`), so a run that updates the index prints one harmless `warning: failed to save last-use data`; accepted. Cargo's workspace search fails with `Permission denied` when an ANCESTOR of the root holds a `Cargo.toml` the jail cannot read (the root is `rwx`, its parents are not); a project nested inside another crate needs `[workspace]` in its manifest. The integrity check lives in `runtime/tools/integrity.rs`: `snapshot(root)` and `compare` return `Change{path, kind, before, after, mode}` (text kept up to 64 KiB, links told by target, bigger files compared by digest); `run_jailed` returns them as `Ran.changes` and X-d1 must warn with a diff and offer to put the old text back (the `before` bytes are enough). Watched: `.git` (when a file), `.git/config`, `.git/hooks/*` (64 first names), `.gitmodules`.
- Accepted residual risk: a `.env` or any other secret file INSIDE the `rwx` root is readable by the toolchain (the root is readable as a whole), and so are in-root hooks until the integrity check above runs.

Env scrub: `env_clear`, then:
- passed through: `PATH` (relative entries and entries under the root removed), `HOME`, `USER`, `LANG`, `LC_ALL`, and `CARGO_HOME`, `RUSTUP_HOME`, `GOPATH`, `GOROOT`, `GOCACHE` and `GOMODCACHE` when they are set;
- set to fixed values: `TMPDIR` (private), `TERM=dumb`, `CI=1`, `NO_COLOR=1`, `CARGO_TERM_COLOR=never`, `CARGO_TERM_PROGRESS_WHEN=never`, `PYTHONUNBUFFERED=1`, `GIT_TERMINAL_PROMPT=0`;
- never passed: the session token, `SSH_AUTH_SOCK`, `GPG_*`, `DISPLAY`, `WAYLAND_DISPLAY`, `DBUS_SESSION_BUS_ADDRESS`, or any `*_TOKEN`/`AWS_*`.

The program is resolved through the scrubbed PATH only. A binary under the root is refused, except the pytest venv rule.

## Allow-list (`core/ai/tools/allow.rs`, pure table)

| Program | Verbs | Denied flags or forms |
|---|---|---|
| cargo | build, check, test, clippy, fmt (only with `--check`), run, doc | `--config`, `-Z`, `--manifest-path`, `--target-dir`, `--fix`, `--allow-dirty`, `install`, `add` |
| npm, pnpm, yarn | test, `run <script>`, install, ci (no package args). yarn/pnpm `run` only for a script declared in the project's `package.json` (no bin fallback) | `-g`/`--global` (also in a short bundle, `-gD`), `--location`, `--prefix`, `--registry`, `--config`/`--config.*`, `--store-dir`, `--global-folder`, `exec`, `dlx`, `x` |
| pytest | (any args) | `--rootdir` or `-c` pointing outside the root, any `@argsfile` word |
| go | test, vet, build, run | `-modfile`, `-toolexec`, `-exec`, `mod`, `install`, `get` |

For every program, an argument that is an absolute path or contains `..` is refused, and so is a `cwd` outside the root. The allow-list bounds the VERB, not the behaviour: the real controls are the per-command `y`, the jail and the tier-B warnings.

The capture buffer keeps a 4 KiB head and a 28 KiB tail ring, with `[… N bytes omitted …]` between them. The output is then decoded as lossy UTF-8, passed through `clean` and `redact`, and wrapped with `label::wrap`.

## Git (`core/ai/tools/git.rs` pure + `runtime/tools/git.rs`)

Every invocation:
- `git -c core.hooksPath=<git-dir>/hooks -c core.fsmonitor=false -c core.untrackedCache=false -c core.pager=cat -c diff.external= -c protocol.allow=never -c credential.helper= -c core.sshCommand=false -c gpg.program=false --no-pager`. The hooks path is `<common-dir>/hooks`: the `GitDir` (`GitDir::new(git_dir, common_dir)`, both absolute and clean) holds `rev-parse --absolute-git-dir` and `--git-common-dir` from the G-b preflight, and every argv builder takes it. In a linked worktree the git dir is `.git/worktrees/<name>` and has no hooks; the common dir does. Status also passes `--untracked-files=normal --renames` so the user's config cannot change what is listed.
- Env: `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_SYSTEM=/dev/null`, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_ATTR_NOSYSTEM=1`, `GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true`, and `GIT_OPTIONAL_LOCKS=0` for the read verbs.
- **Hooks (user decision, 2026-10-26):** `core.hooksPath` is pinned to the repo's own `.git/hooks`. The write tools refuse to touch it, whereas a worktree hook dir (husky, `.githooks`) can be written by the model, so it never runs from lazarobox. If the repo sets another `core.hooksPath`, the commit review shows `hooks_notice`: "this repo sets core.hooksPath to X; lazarobox runs only the repo hooks folder". G-b reads the configured value.
- **Global config (user decision, 2026-10-26):** it is never loaded (`GIT_CONFIG_GLOBAL=/dev/null`): `include.path`, filters, textconv and gpg settings in it are attack surface. A commit takes an `Identity` (name, email: non-empty, at most 256 bytes, one clean line, email with `@` and no whitespace) and passes `-c user.name=<name> -c user.email=<email>`.
- **Identity source (user decision, 2026-10-27, replaces "global only"):** the identity is the repo's EFFECTIVE identity: the G-b preflight runs `git config user.name` / `user.email` inside the repo (outside the jail, global config in scope, so a repo-level override wins) and passes the values to `Identity::new`. G-a stays pure. An invalid value refuses the commit with a fixed error.
- **Log history (user decision, 2026-10-27):** `git log` keeps history simplification together with the secret exclude pathspecs, so a commit that touches only secret files is hidden from the log. `--full-history` is never passed (a test pins it).
- **G-b checklist (record, not implemented in G-a):** assert the cleared environment and git >= 2.32 (`GIT_CONFIG_GLOBAL` needs it); never pass through `GIT_EXTERNAL_DIFF`, `GIT_PAGER`, `GIT_ASKPASS`, `GIT_SSH`, `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_*`/`GIT_CONFIG_VALUE_*`, `GIT_CONFIG_PARAMETERS`, `GIT_DIR`, `GIT_WORK_TREE`; treat truncated status output as truncated (do not parse a cut record, say so to the model).

The git spec is the run spec without the caches, plus:
- ro_files `$XDG_CONFIG_HOME/git/{ignore,attributes}` for ignores (the global config file is NOT granted: identity travels as `-c`, see above);
- rw on `--absolute-git-dir` and `--git-common-dir`, resolved by a jailed read-only `rev-parse` preflight. Linked worktrees keep their gitdir outside the root.

| Tool | Toggle | Ask | argv |
|---|---|---|---|
| `git_status` | git | none | `status --porcelain=v2 -z --branch --ignore-submodules=all --untracked-files=normal --renames -- . <excludes>`. Entries whose path or old path is a secret name or lives in a secret folder are dropped. |
| `git_diff{staged?, path?}` | git | on Cloud (shows file text) | `diff [--cached] --no-ext-diff --no-textconv --ignore-submodules=all --no-color -- <:(literal)path\|.> <excludes>` |
| `git_log{max_count?, path?}` | git | none | `log -n <1..50, default 20> --no-color --no-show-signature --format=%H%x1f%an%x1f%aI%x1f%s -- [:(literal)path] <excludes>` (never `-p`) |
| `git_add{paths[]}` | git_write | one-line overlay (paths + diffstat), or the review view when that does not fit | `add -- :(literal)p… <excludes>`. A secret name or a path inside a secret folder is refused; the excludes keep `.` or a folder from staging the secrets below it. |
| `git_commit{message}` | git_write | review view: the message, the full `--cached` diff, and the notice "signing disabled for this commit" | `-c commit.gpgsign=false -c user.name=<n> -c user.email=<e> commit --cleanup=whitespace -m <msg>`. Refused when the index holds a secret-listed path, so the review stays honest. Hooks run inside the jail. No AI trailer is added. |

`<excludes>` are `:(exclude,glob,icase)` pathspecs (`icase` matches the case-insensitive path guard): `**/<name>` for every SECRET_FILES entry plus `.env`, `.env.*` and `.lbx-*.tmp`, and `**/<dir>/**` for every SECRET_DIRS and CREDENTIAL_DIRS entry. A path given to a verb is refused when it has a `.git` component, a secret name or a secret folder. Model-facing text (status, log) replaces control, format and bidi characters with `?`. **Residual risk:** filter drivers (clean/process, git-lfs) defined in the repo config stay active and run; this rests on the jail, not on git settings. Every git tool is refused while this key has a batch staged or proposed.

## Data Flow

```
model ─ create_dir/write_file/edit_file ─→ host: resolve_write + content rules
        → Stages[key].stage()  (no ask, no disk)
model ─ read_file/glob/grep/list_dir ─→ fs (+ StagedView snapshot, "pending" marker)
model ─ propose_batch ─→ Stages.seal(key) → BatchReview (Arc, BatchId)
        → ApprovalAsk::Batch → app review view (coverage + 800 ms) → y
        → Stages.apply(BatchId) → preflight/drift → journaled writes → Vec<Change>
   n/Esc → discard          turn cancelled / CLI timeout → orphan (pane badge)
orphan: user key → app Effect::BatchReview/Apply/Discard → Chats → Stages
model ─ run_command/git_* ─→ refused if Stages[key] non-empty ─→ ask (y)
        → RunGate(1/key, 2 global) → runner: helper(--rwx root) → pipe → ring
        → RunOutcome → redact + wrap
pane close / app quit → Stages.discard(key) + notice
```

### Host seam for the write family (W-f)

`host.rs` gets only dispatch hooks (about 27 net lines); the logic is the child module `runtime/tools/host/files.rs`.

- `TurnScope` gains `families: FamilySet` (default `LEGACY`) and `stage_key: Option<StageKey>` (default `None`); `ToolHost` owns `stages: Arc<Mutex<Stages>>`; `ToolHost::env(origin)` replaces the inline `ToolEnv` build. `runtime/chat.rs` fills the two scope fields (3 lines).
- `execute` hooks: `files::gate` (second family check, same text as an unknown tool), `files::seal` before the ask (`ProposeBatch` builds `ApprovalAsk::Batch` itself; `ask_for` returns `None` for the four write calls), `files::answered` for the decision, `files::run` for every call that needs no answer (staging, and the read tools over `Stages::view(key)` whenever Write is on).
- The `BatchId` is NOT put in the ask: the answer returns to the very `execute` future that sealed the batch and holds the id, and `Stages::apply(id, ..)` is generation-checked, so a stale answer is `Stale`. W-g needs an id-carrying route only for orphans (no live future).
- `ToolEnv` is unchanged (12 literals); the view travels as an argument: `tools::run_over(call, env, limits, Option<StagedView>)`, `run` is `run_over(.., None)`.
- Why a seam inside `host.rs` and not outside: the ask must be registered before the `ToolCall` is announced and the result must go through `Owed`, so `propose_batch` has to live in `execute`'s flow. `stage_with` became generic over the error (`edit_file` returns `EditError`) and `stage::disk_bytes` wraps `Disk::probe`, so `files.rs` names neither the `write` nor the `apply` module (no audit change).
- A non-applied result (`Drifted`, `Invalid`, `Failed`, `Unsupported`) and every denial discard the batch, so the model can stage again.

## Interfaces / Contracts

```rust
pub enum Family { Read, Search, Write, Exec, Git, GitWrite, Pane }
#[derive(Clone, Copy)] pub struct FamilySet(u8); // LEGACY = {Read, Pane}
pub enum ArgKind { Str { max: usize }, Int { min: i64, max: i64 }, Bool, StrList { items: usize, each: usize } }
pub struct ArgDef { pub name: &'static str, pub kind: ArgKind, pub required: bool, pub gate: Option<Family> }
pub struct ToolDef { pub name: &'static str, pub description: &'static str, pub access: Access, pub family: Family, pub args: &'static [ArgDef] }
pub fn registry_with(external: &[ExternalTool], set: FamilySet) -> Vec<ToolSpec>;
pub fn parse_call_in(name: &str, args: &Value, ext: &Qualified, set: FamilySet) -> Result<Call, ToolError>;
pub enum StageKey { Pane(PaneId) /* S7 adds Worker{..} */ }
pub struct BatchId { pub key: StageKey, pub gen: u64 }
pub struct Change { pub path: PathBuf, pub kind: ChangeKind /* Created|Overwritten|Edited|DirCreated */, pub bytes: u64, pub added: u32, pub removed: u32 }
pub enum ApplyResult { Applied(Vec<Change>), Drifted(Vec<PathBuf>), Failed { rolled_back: bool }, Partial { changed: Vec<PathBuf> } }
pub struct RunOutcome { pub argv: Vec<String>, pub exit: Option<i32>, pub signal: Option<i32>, pub timed_out: bool,
    pub cancelled: bool, pub truncated: bool, pub total_bytes: u64, pub duration: Duration, pub digest: String /* ≤ 2 KiB cleaned tail, for S8 Evidence */ }
```

The `Call` variants are added per slice: `Glob`, `Grep`, `CreateDir`, `WriteFile`, `EditFile`, `ProposeBatch`, `RunCommand`, `GitStatus`, `GitDiff`, `GitLog`, `GitAdd` and `GitCommit`. P0 adds only `ReadFile{path, lines: Option<Lines>}`, so later slices do not edit host.rs:293. The `ApprovalAsk` variants are `Search`, `Batch(Arc<BatchReview>)`, `Exec`, `GitDiff`, `GitAdd` and `GitCommit(Arc<CommitReview>)`. Every new ask routes `Prompt` in every mode: no AutoRun and no AutoType.

Ollama: `MAX_ROUNDS` becomes `max_rounds(set)`, which is 24 when Search, Write, Exec, Git or GitWrite is on, and 8 otherwise.

## File Changes (by slice; real sizes ≈ 2.4–3x naive)

| Slice | Files | Real |
|---|---|---|
| P0a | `core/ai/tools/registry.rs` (new), `tools/mod.rs`, a 0.18.0 `tools/list` golden written FIRST, `host.rs:293` (ReadFile field) | ~450 |
| P0b | `FamilySet` plumbing at LEGACY: `mcp.rs` Server, `chat/stream.rs`, `runtime/mcp/session.rs`, `runtime/chat.rs:716`, `chat/adapters.rs:287`, `ChatRequest.families` | ~350 |
| T | `core/ai/config.rs` (5 keys), `core/config_edit.rs`, `core/menu/*` rows, app → `ChatRequest`, `runtime/chat/ollama.rs` `max_rounds` | ~450 |
| M1 S-a/b/c | `Cargo.toml` (+regex), `tools/glob.rs` (glob→anchored regex), `runtime/tools/search.rs` (new), `fs.rs` (paging, .git/config deny), `chat/approval.rs` `Search`, `app/tool.rs`, `app/run.rs` | ~1,200 / 3 PRs |
| M2 W-a..h | `runtime/tools/open.rs` (write primitives), `tools/content.rs`, `tools/paths.rs` (`resolve_write`), `tools/stage.rs`, `tools/edit.rs`, `tools/diff.rs` (+similar), `core/ai/chat/review.rs`, `app/review.rs`, `ui/components/review.rs`, `ui/components/chat.rs` (badge), `runtime/tools/stage.rs`, `runtime/tools/apply.rs`, `host.rs`, `runtime/chat.rs` (discard on close, orphan effects), `app/notes.rs` quit notice | ~4,400 / 11 PRs |
| M3 X-a..d | `tools/allow.rs`, `tools/run.rs` (Capture, RunOutcome), `runtime/tools/runner.rs`, `core/ai/toolchain.rs`, `core/ai/jail.rs` (rwx + flags), `runtime/jail.rs` (Execute for rwx, optional BindTcp, rlimits), `host.rs`, approval/app | ~2,800 / 7 PRs |
| M4 G-a..c | `tools/git.rs`, `runtime/tools/git.rs`, `host.rs`, approval/app/review (commit) | ~1,400 / 4 PRs |
| M5 | `README.md`, threat model, CHANGELOG per release | ~600 spread |

Sequencing:
- P0a and P0b land on `main` before S1c-1; S1c-1 maps `registry_for(Toolset)` to `registry_with(.., toolset.families())`.
- T, M1, and the pure M2/M3 cores (W-a..e, X-a, X-b, the X-c jail) do not touch host.rs.
- W-f..h, X-d and M4 wiring no longer wait for S1c-2: coding-tools lands in host.rs first and the S1 chain rebases onto it. Each of these slices still spikes first for a seam outside host.rs.

## Testing Strategy (strict TDD, `cargo test`)

| Layer | What | Approach |
|---|---|---|
| Characterization | `tools/list` bytes and parse errors of the 4 tools; all-toggles-off behaviour | A golden captured on 0.18.0 BEFORE the P0 refactor; it must stay green across every slice |
| Unit (pure) | ArgDef parse and order, gates, clamping; glob→regex; content rules; resolve_write deny; edit unique match; stage state machine and caps; diff hunks; ReviewDoc layout and coverage on resize; allow-list table; git argv and porcelain v2 parse; Capture ring | Table tests, written red first |
| Integration (Linux, tempfile) | Apply: drift, symlink swap, nlink>1, rollback with a fault at step k, a failing undo → Partial | A `WriteOps` fault injector |
| Integration (runner) | Group kill of grandchildren, timeout, cancel by drop, ring truncation, env scrub | `/bin/sh` scripts through the unexported spawn core (below the allow-list) |
| Jail | rwx exec works in the root; `~/.ssh` unreadable; listen allowed | Skip when ABI < 3 (the existing helper exits 125 for `--min-abi`) |
| Git | A planted `core.fsmonitor`/`diff.external` in `.git/config` must NOT create its marker file; secret excludes; gpgsign off; commit refused on a staged `.env` | A temp repo, skipped when git is missing |
| UI | Review view, gate counter, badge, warnings | TestBackend + insta, fake clock for the arm delay |

## Migration / Rollout

No data migration. New `[ai.tools]` booleans default to false, and older builds ignore them. Releases: 0.19 (P0, T, M1), 0.20 (M2, then the go/no-go on review UX), 0.21 (M3), 0.22 (M4). Every PR reverts on its own.

## Resolved by the user (2026-10-22)

- **Sequencing:** keep feature-branch-chain for agent-orchestrator. P0, T, M1 and the pure M2/M3 cores go ahead now. The `host.rs` slices (W-f..h, X-d, the M4 wiring) do not wait for S1c-2: coding-tools lands in `host.rs` first and the S1 chain rebases. Each slice still spikes first for a seam outside `host.rs`. S1c-2 is not merged early.
- **Pathname UNIX sockets (D15):** accepted as a documented residual risk in v1. Seccomp hardening is follow-up work, not a v1 blocker.
- **Open TCP bind in a toolchain run (D15), decided consciously in the X-c3 review:** every toolchain run gets `NetBind::Open`, because dev servers (`cargo run`, `go run`, `npm run dev`, `pytest` with a live server) must listen. The risk is port squatting: a program (or a dependency it builds) can bind a well-known local port and keep it after the run's own work is done, or take a port another local service expects. The process group is killed when the run ends, so a squatter does not outlive the run, but during the run it can answer on that port. Accepted for v1; a narrower rule (a port range, or a seccomp filter) is follow-up work together with the UNIX socket hardening above. No code change.
- **Git read verbs on a Cloud engine:** they need an explicit `y`, the same as `read_file`. On a Local engine they run without asking.
- **Design adjustments 1-14** in the design hand-back are all accepted: toggles in slice T, the `GitWrite` family, the cap checked at stage time, `list_dir` sees staged content, the outside-HOME refusal, ZWJ/ZWNJ refused, flag-level denials, listening sockets and `/usr` rx in the toolchain jail, read-only `~/.gitconfig` for git only, commit refused on secret-listed index paths, the per-kind concurrency caps, `run_command` offered but refused without a jail, workers never leave orphans, and the review keys.
- **`edit_file` on a Cloud engine asks like `read_file`** (it is a content oracle), once per file per turn (the turn's `allowed` set, filled by an allowed read or edit ask); Local never asks.
- **The orphan review key** is Ctrl+G. W-h still checks it against the chat key table.
- **The model is told** about a user-side apply or discard of an orphan on its next turn, through a system line in the request.
- **Archive order:** `agent-tools` MUST be archived before `coding-tools`.

## Open Questions

- [ ] The S8 meaning of `Evidence.digest` (assumed: the cleaned tail text, not a hash). Confirm with the orchestrator owner.
- [ ] How Landlock handles pathname UNIX sockets on the target kernels (D15). Decide on seccomp hardening after X-c.
- [ ] The Ollama path collapses duplicate JSON keys before parse (stream.rs:328). The review shows the final content, so this cannot bypass the review; P0 should still add the raw-key check.
- [ ] macOS `run_command` (user decision needed): the toolchain jail needs Seatbelt support for `rwx` (allow `file-write*` and `process-exec` in one subpath, plus toolchain roots), BindTcp and rlimits. X-c1 refuses `rwx` on macOS (`ExecFault::Macos`, `SbplError::Rwx`) and X-d1 already refuses a run without a jail; a macOS slice is not planned.
- [ ] pytest installed with `pip --user` needs read access to its site-packages (for example `~/.local/lib/python3.x`), which the spec does not grant; only the program file or its venv is `rx`. Check with a manual run in X-c3 or Z.

## Follow-up notes

- **Stronger alternative (integrity check, X-c3):** deny writes to `.git/config`, `.git/hooks`, `.gitmodules` and an existing in-root `core.hooksPath` inside the jail (`rw_except`), turning detection into prevention. The detector (`integrity.rs`) stays as the fail-closed net for what the deny list cannot name (a `hooksPath` created later, an include chain, a submodule's git dir).
